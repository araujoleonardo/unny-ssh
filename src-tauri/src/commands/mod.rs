pub mod connections;
pub mod docker;
pub mod files;
pub mod lifecycle;
pub mod transfers;
use crate::{
    ipc::{AppError, Result},
    services::{
        streams::{StreamEvent, Streams},
        vault::{Vault, VaultStatus},
    },
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tauri::{State, ipc::Channel};
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct AppState {
    pub vault: Mutex<Vault>,
    pub access: std::sync::atomic::AtomicU64,
    pub docker: Mutex<crate::services::docker::Docker>,
    pub files: Mutex<crate::services::files::Files>,
    pub transfers: Mutex<crate::services::transfers::Transfers>,
    pub sessions: Mutex<crate::services::sessions::Sessions>,
    pub imports: Mutex<crate::services::imports::Imports>,
    pub profile_deletes: Mutex<std::collections::HashMap<Uuid, (Uuid, u64, std::time::Instant)>>,
    pub confirmations: Mutex<crate::services::confirmation::ResetConfirmation>,
    pub streams: Mutex<Streams>,
}
impl AppState {
    pub fn authorize(&self, generation: u64) -> Result<()> {
        let current = self.access.load(std::sync::atomic::Ordering::SeqCst);
        if current == 0 {
            return Err(AppError::new(
                "VAULT_LOCKED",
                "Desbloqueie o cofre para continuar.",
            ));
        }
        if current != generation {
            return Err(AppError::new("STALE_GENERATION", "Esta operação expirou."));
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultRequest {
    Create {
        password: String,
    },
    Unlock {
        password: String,
    },
    Lock,
    ChangePassword {
        old_password: String,
        new_password: String,
    },
    Reset {
        password: String,
        confirmation: String,
        confirmation_token: Uuid,
    },
}
#[tauri::command]
pub fn vault_status(state: State<'_, Arc<AppState>>) -> Result<VaultStatus> {
    Ok(state.vault.try_lock().map_err(|_| busy())?.status())
}
#[tauri::command]
pub async fn vault_action(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<VaultStatus> {
    let request: VaultRequest = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    // KDF and file sync run on the blocking pool, never on the UI/async executor.
    tauri::async_runtime::spawn_blocking(move || {
        let mut vault = state.vault.lock().map_err(|_| AppError::io())?;
        let operation = match &request {
            VaultRequest::Create { .. } => "VAULT_CREATE",
            VaultRequest::Unlock { .. } => "VAULT_UNLOCK",
            VaultRequest::Lock => "VAULT_LOCK",
            VaultRequest::ChangePassword { .. } => "VAULT_ROTATE",
            VaultRequest::Reset { .. } => "VAULT_RESET",
        };
        let started = std::time::Instant::now();
        let result = match request {
            VaultRequest::Create { password } => vault.create(&Zeroizing::new(password)),
            VaultRequest::Unlock { password } => vault.unlock(&Zeroizing::new(password)),
            VaultRequest::Lock => {
                if state
                    .files
                    .lock()
                    .map_err(|_| AppError::io())?
                    .contexts
                    .values()
                    .any(|ctx| ctx.documents.lock().is_ok_and(|docs| !docs.is_empty()))
                {
                    return Err(AppError::new(
                        "DRAFT_CAPTURE_REQUIRED",
                        "Use o coordenador para proteger documentos antes de bloquear.",
                    ));
                }
                lifecycle::invalidate(&state)?;
                Ok(vault.lock())
            }
            VaultRequest::ChangePassword {
                old_password,
                new_password,
            } => {
                vault.change_password(&Zeroizing::new(old_password), &Zeroizing::new(new_password))
            }
            VaultRequest::Reset {
                password,
                confirmation,
                confirmation_token,
            } => {
                let password = Zeroizing::new(password);
                state
                    .confirmations
                    .lock()
                    .map_err(|_| AppError::io())?
                    .consume(confirmation_token, vault.status().generation)?;
                vault.reset(&password, &confirmation)
            }
        };
        vault.audit(
            operation,
            result.as_ref().map(|_| "OK").unwrap_or_else(|e| e.code),
            started.elapsed().as_millis(),
        );
        let status = result?;
        lifecycle::invalidate(&state)?;
        state.access.store(
            if status.state == "unlocked" {
                status.generation
            } else {
                0
            },
            std::sync::atomic::Ordering::SeqCst,
        );
        Ok(status)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub fn domain_probe(context: serde_json::Value, state: State<'_, Arc<AppState>>) -> Result<()> {
    let context: crate::ipc::ResourceContext =
        serde_json::from_value(context).map_err(|_| AppError::invalid())?;
    state.authorize(context.generation)?;
    state
        .sessions
        .lock()
        .map_err(|_| AppError::io())?
        .authorize(context.session_id, context.generation)?;
    Ok(())
}
#[tauri::command]
pub fn stream_open(
    generation: u64,
    channel: Channel<StreamEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    state.authorize(generation)?;
    let mut streams = state.streams.lock().map_err(|_| AppError::io())?;
    let id = streams.open(generation, channel)?;
    streams.progress(
        id,
        crate::ipc::OperationState::Running,
        0,
        None,
        std::time::Instant::now(),
    )?;
    Ok(id)
}
#[tauri::command]
pub fn stream_ack(
    operation_id: Uuid,
    generation: u64,
    sequence: u64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    state.authorize(generation)?;
    state
        .streams
        .lock()
        .map_err(|_| AppError::io())?
        .ack(operation_id, generation, sequence)
}
#[tauri::command]
pub fn stream_cancel(
    operation_id: Uuid,
    generation: u64,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    state.authorize(generation)?;
    state
        .streams
        .lock()
        .map_err(|_| AppError::io())?
        .cancel(operation_id, generation)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_contracts() {
        assert!(
            serde_json::from_str::<VaultRequest>(r#"{"action":"unlock","password":"x","extra":1}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<crate::ipc::ResourceContext>(
                r#"{"sessionId":"invalid","generation":1}"#
            )
            .is_err()
        );
        let error = AppError::new("VAULT_LOCKED", "Desbloqueie.");
        let json = serde_json::to_value(error).unwrap();
        assert!(json["correlationId"].is_string());
    }
}

#[tauri::command]
pub fn reset_challenge(state: State<'_, Arc<AppState>>) -> Result<Uuid> {
    let vault = state.vault.try_lock().map_err(|_| busy())?;
    Ok(state
        .confirmations
        .lock()
        .map_err(|_| AppError::io())?
        .issue(vault.status().generation))
}

fn busy() -> AppError {
    AppError::new(
        "BUSY",
        "O cofre está processando outra operação. Aguarde e tente novamente.",
    )
}
