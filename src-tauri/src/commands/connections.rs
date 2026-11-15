use super::AppState;
use crate::{
    ipc::{AppError, Result},
    services::{
        imports::CredentialInput,
        profiles::{AuthMode, Profile, ProfileInput, SecretPolicy},
        sessions::{
            self, SessionCommand, SessionInfo, SessionRef,
            terminal::{self, TerminalCommand, TerminalEvent},
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{io::Read, sync::Arc, time::Instant};
use tauri::{State, ipc::Channel};
use uuid::Uuid;
fn parse<T: serde::de::DeserializeOwned>(request: serde_json::Value) -> Result<T> {
    serde_json::from_value(request).map_err(|_| AppError::invalid())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Generation {
    generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileRef {
    generation: u64,
    profile_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Put {
    generation: u64,
    profile: ProfileInput,
    secret_policy: SecretPolicy,
    credential: Option<CredentialInput>,
}
#[tauri::command]
pub async fn profiles_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Profile>> {
    let request: Generation = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .profiles(request.generation)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn profile_put(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Profile> {
    let request: Put = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut vault = state.vault.lock().map_err(|_| AppError::io())?;
        vault.authorize(request.generation)?;
        let secret = if matches!(request.secret_policy, SecretPolicy::Save) {
            Some(state.imports.lock().map_err(|_| AppError::io())?.resolve(
                request.generation,
                request.profile.auth_mode,
                request.credential.ok_or_else(AppError::invalid)?,
            )?)
        } else {
            None
        };
        vault.put_profile(
            request.generation,
            request.profile,
            request.secret_policy,
            secret,
        )
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn profile_duplicate(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Profile> {
    let request: ProfileRef = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .duplicate_profile(request.generation, request.profile_id)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub fn profile_delete_prepare(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    let request: ProfileRef = parse(request)?;
    state
        .vault
        .try_lock()
        .map_err(|_| super::busy())?
        .profile(request.generation, request.profile_id)?;
    let token = Uuid::new_v4();
    let mut confirmations = state.profile_deletes.lock().map_err(|_| AppError::io())?;
    confirmations.retain(|_, (_, _, created)| created.elapsed().as_secs() < 300);
    if confirmations.len() >= 32 {
        return Err(AppError::new(
            "BUSY",
            "Aguarde a expiração das confirmações anteriores.",
        ));
    }
    confirmations.insert(
        token,
        (request.profile_id, request.generation, Instant::now()),
    );
    Ok(token)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Delete {
    generation: u64,
    profile_id: Uuid,
    token: Uuid,
}
#[tauri::command]
pub async fn profile_delete(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let request: Delete = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut vault = state.vault.lock().map_err(|_| AppError::io())?;
        vault.authorize(request.generation)?;
        let proof = state
            .profile_deletes
            .lock()
            .map_err(|_| AppError::io())?
            .remove(&request.token);
        if !proof.is_some_and(|(id, generation, created)| {
            id == request.profile_id
                && generation == request.generation
                && created.elapsed().as_secs() < 300
        }) {
            return Err(AppError::new(
                "CONFIRMATION_EXPIRED",
                "Confirmação expirada. Confira a conexão e tente novamente.",
            ));
        }
        vault.delete_profile(request.generation, request.profile_id)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    id: Uuid,
    name: String,
}
#[tauri::command]
pub async fn key_import(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<ImportResult>> {
    use tauri_plugin_dialog::DialogExt;
    let request: Generation = parse(request)?;
    state
        .vault
        .try_lock()
        .map_err(|_| super::busy())?
        .authorize(request.generation)?;
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<Option<(String, zeroize::Zeroizing<String>)>> {
            let Some(file) = app
                .dialog()
                .file()
                .set_title("Selecionar chave privada SSH")
                .blocking_pick_file()
            else {
                return Ok(None);
            };
            let path = file.into_path().map_err(|_| AppError::invalid())?;
            let file = std::fs::File::open(&path).map_err(|_| AppError::io())?;
            if !file.metadata().map_err(|_| AppError::io())?.is_file() {
                return Err(AppError::invalid());
            }
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            file.take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| AppError::io())?;
            if bytes.len() > 1024 * 1024 {
                return Err(AppError::new(
                    "INVALID_KEY",
                    "A chave excede o limite de 1 MiB.",
                ));
            }
            let text = zeroize::Zeroizing::new(
                std::str::from_utf8(&bytes)
                    .map_err(|_| {
                        AppError::new(
                            "INVALID_KEY",
                            "Selecione uma chave privada SSH em formato texto.",
                        )
                    })?
                    .to_owned(),
            );
            if !text.contains("PRIVATE KEY") {
                return Err(AppError::new(
                    "INVALID_KEY",
                    "O arquivo selecionado não contém uma chave privada.",
                ));
            }
            Ok(Some((
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                text,
            )))
        },
    )
    .await
    .map_err(|_| AppError::io())??;
    let Some((name, contents)) = result else {
        return Ok(None);
    };
    let vault = state.vault.try_lock().map_err(|_| super::busy())?;
    vault.authorize(request.generation)?;
    let id = state
        .imports
        .lock()
        .map_err(|_| AppError::io())?
        .insert(request.generation, contents)?;
    Ok(Some(ImportResult { id, name }))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Connect {
    generation: u64,
    profile_id: Uuid,
    credential: Option<CredentialInput>,
}
#[tauri::command]
pub async fn session_connect(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<SessionInfo> {
    let request: Connect = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let vault = state.vault.lock().map_err(|_| AppError::io())?;
        let profile = vault.profile(request.generation, request.profile_id)?;
        let secret = if let Some(input) = request.credential {
            Some(state.imports.lock().map_err(|_| AppError::io())?.resolve(
                request.generation,
                profile.fields.auth_mode,
                input,
            )?)
        } else {
            vault.credential(request.generation, &profile)?
        };
        if profile.fields.auth_mode != AuthMode::Agent && secret.is_none() {
            return Err(AppError::new(
                "CREDENTIAL_REQUIRED",
                "Informe a credencial para esta conexão.",
            ));
        }
        let known = vault.known_host(
            request.generation,
            &profile.fields.host,
            profile.fields.port,
        )?;
        sessions::start(
            state.clone(),
            request.generation,
            profile.fields,
            secret,
            known,
        )
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub fn sessions_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<SessionInfo>> {
    let request: Generation = parse(request)?;
    state.authorize(request.generation)?;
    Ok(state
        .sessions
        .lock()
        .map_err(|_| AppError::io())?
        .entries
        .values()
        .filter(|e| e.info.generation == request.generation)
        .map(|e| e.info.clone())
        .collect())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Decision {
    session_id: Uuid,
    generation: u64,
    challenge_id: Uuid,
    accept: bool,
    revision: String,
}
#[tauri::command]
pub fn session_confirm_host(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let request: Decision = parse(request)?;
    state.authorize(request.generation)?;
    let mut sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    let entry = sessions.authorize(request.session_id, request.generation)?;
    if entry.info.challenge.as_ref().map(|c| c.id) != Some(request.challenge_id) {
        return Err(AppError::new(
            "CONFIRMATION_EXPIRED",
            "Desafio expirado ou já respondido.",
        ));
    }
    entry
        .tx
        .try_send(SessionCommand::Confirm {
            challenge: request.challenge_id,
            accept: request.accept,
            revision: request.revision,
        })
        .map_err(|_| super::busy())?;
    if let Some(entry) = sessions.entries.get_mut(&request.session_id) {
        entry.info.challenge = None;
        entry.info.state = "connecting";
    }
    Ok(())
}
#[tauri::command]
pub async fn session_disconnect(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    close_session(request, state, false).await
}
#[tauri::command]
pub async fn session_close(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    close_session(request, state, true).await
}
async fn close_session(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
    remove: bool,
) -> Result<()> {
    let request: SessionRef = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.authorize(request.generation)?;
        {
            let mut sessions = state.sessions.lock().map_err(|_| AppError::io())?;
            sessions.authorize(request.session_id, request.generation)?;
            sessions
                .entries
                .get_mut(&request.session_id)
                .ok_or_else(AppError::invalid)?
                .info
                .state = "disconnected".into();
        }
        super::lifecycle::quiesce(&state, Some(request.session_id))?;
        {
            let mut sessions = state.sessions.lock().map_err(|_| AppError::io())?;
            sessions.disconnect(request.session_id, request.generation)?;
            if remove {
                sessions.entries.remove(&request.session_id);
            }
        }
        crate::services::docker::forget(&state, request.session_id);
        if remove {
            state
                .files
                .lock()
                .map_err(|_| AppError::io())?
                .contexts
                .remove(&request.session_id);
        }
        Ok(())
    })
    .await
    .map_err(|_| AppError::io())?
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Open {
    session_id: Uuid,
    generation: u64,
    cols: u32,
    rows: u32,
}
#[tauri::command]
pub async fn terminal_open(
    request: serde_json::Value,
    channel: Channel<TerminalEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    let request: Open = parse(request)?;
    terminal::dimensions(request.cols, request.rows)?;
    let (reply, wait) = tokio::sync::oneshot::channel();
    {
        state.authorize(request.generation)?;
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        let session = sessions.authorize(request.session_id, request.generation)?;
        if session.info.state != "connected" {
            return Err(sessions::unknown());
        }
        session
            .tx
            .try_send(SessionCommand::OpenTerminal {
                request: terminal::OpenTerminal {
                    command: None,
                    cols: request.cols,
                    rows: request.rows,
                    channel,
                },
                reply,
            })
            .map_err(|_| super::busy())?;
    }
    wait.await.map_err(|_| sessions::unknown())?
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum TerminalAction {
    Write { bytes: Vec<u8> },
    Resize { cols: u32, rows: u32 },
    Ack { sequence: u64 },
    Close,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TerminalRequest {
    session_id: Uuid,
    generation: u64,
    terminal_id: Uuid,
    command: TerminalAction,
}
#[tauri::command]
pub fn terminal_action(request: serde_json::Value, state: State<'_, Arc<AppState>>) -> Result<()> {
    let request: TerminalRequest = parse(request)?;
    state.authorize(request.generation)?;
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    sessions.authorize(request.session_id, request.generation)?;
    let terminal = sessions
        .terminals
        .get(&request.terminal_id)
        .ok_or_else(sessions::unknown)?;
    if terminal.session_id != request.session_id || terminal.generation != request.generation {
        return Err(sessions::expired());
    }
    let command = match request.command {
        TerminalAction::Write { bytes } => {
            if bytes.is_empty() || bytes.len() > 65536 {
                return Err(AppError::invalid());
            }
            TerminalCommand::Input(bytes)
        }
        TerminalAction::Resize { cols, rows } => {
            terminal::dimensions(cols, rows)?;
            TerminalCommand::Resize(cols, rows)
        }
        TerminalAction::Ack { sequence } => TerminalCommand::Ack(sequence),
        TerminalAction::Close => {
            terminal.cancel.cancel();
            return Ok(());
        }
    };
    terminal.tx.try_send(command).map_err(|_| {
        AppError::new(
            "BACKPRESSURE",
            "Entrada temporariamente cheia. Aguarde o terminal consumir os dados.",
        )
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClipboardWrite {
    generation: u64,
    text: String,
}
#[tauri::command]
pub async fn clipboard_write(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let request: ClipboardWrite = parse(request)?;
    state.authorize(request.generation)?;
    if request.text.len() > 256 * 1024 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "Selecione até 256 KiB para copiar.",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        app.clipboard().write_text(request.text).map_err(|_| {
            AppError::new(
                "CLIPBOARD_UNAVAILABLE",
                "Não foi possível copiar para a área de transferência.",
            )
        })
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn clipboard_read(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let request: Generation = parse(request)?;
    state.authorize(request.generation)?;
    let text = tauri::async_runtime::spawn_blocking(move || {
        app.clipboard().read_text().map_err(|_| {
            AppError::new(
                "CLIPBOARD_UNAVAILABLE",
                "A área de transferência não contém texto disponível.",
            )
        })
    })
    .await
    .map_err(|_| AppError::io())??;
    state.authorize(request.generation)?;
    if text.len() > 256 * 1024 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "Divida a colagem em partes de até 256 KiB.",
        ));
    }
    Ok(text)
}
