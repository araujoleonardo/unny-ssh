use super::AppState;
use crate::{
    ipc::{AppError, Result},
    services::{
        files,
        vault::VaultStatus,
        vault::drafts::{DraftInfo, Identity},
    },
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Buffer {
    session_id: Uuid,
    document_id: Uuid,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lock {
    generation: u64,
    documents: Vec<Buffer>,
}
// Runs on a blocking worker. Let channel tasks signal/close while SSH is still
// alive, then disconnect the transport. Waiting is bounded and never uses a lock
// across the wait; cancellation cannot promise rollback of remote side effects.
pub fn quiesce(state: &AppState, session: Option<Uuid>) -> Result<()> {
    state
        .docker
        .lock()
        .map_err(|_| AppError::io())?
        .cancel(session);
    state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .cancel_owner(session);
    {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        for t in sessions.terminals.values() {
            if session.is_none_or(|id| t.session_id == id) {
                t.cancel.cancel();
            }
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let docker = state
            .docker
            .lock()
            .map_err(|_| AppError::io())?
            .busy(session);
        let terminals = state
            .sessions
            .lock()
            .map_err(|_| AppError::io())?
            .terminals
            .values()
            .any(|t| session.is_none_or(|id| t.session_id == id));
        if !docker && !terminals || std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}
pub fn invalidate(state: &AppState) -> Result<()> {
    state.access.store(0, std::sync::atomic::Ordering::SeqCst);
    quiesce(state, None)?;
    state.docker.lock().map_err(|_| AppError::io())?.clear();
    state.sessions.lock().map_err(|_| AppError::io())?.clear();
    state.transfers.lock().map_err(|_| AppError::io())?.clear();
    state
        .streams
        .lock()
        .map_err(|_| AppError::io())?
        .invalidate();
    state.imports.lock().map_err(|_| AppError::io())?.clear();
    state
        .profile_deletes
        .lock()
        .map_err(|_| AppError::io())?
        .clear();
    state
        .files
        .lock()
        .map_err(|_| AppError::io())?
        .contexts
        .clear();
    Ok(())
}
#[tauri::command]
pub async fn workspace_lock(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<VaultStatus> {
    let r: Lock = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut vault = state.vault.lock().map_err(|_| AppError::io())?;
        vault.authorize(r.generation)?;
        let mut drafts = Vec::new();
        for buffer in r.documents {
            let identity = {
                let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
                Identity::from_profile(
                    &sessions
                        .authorize(buffer.session_id, r.generation)?
                        .info
                        .profile,
                )?
            };
            let ctx = state
                .files
                .lock()
                .map_err(|_| AppError::io())?
                .contexts
                .get(&buffer.session_id)
                .cloned()
                .ok_or_else(AppError::invalid)?;
            drafts.push(files::documents::capture(
                &ctx,
                buffer.document_id,
                buffer.text,
                identity,
            )?);
        }
        // Owned buffers survive even when persistence fails; all network children stop.
        invalidate(&state)?;
        vault.lock_drafts(r.generation, drafts)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Recovery {
    password: String,
    discard: bool,
}
#[tauri::command]
pub async fn workspace_recover(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<VaultStatus> {
    let r: Recovery = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut vault = state.vault.lock().map_err(|_| AppError::io())?;
        let status = vault.recover_lock(&zeroize::Zeroizing::new(r.password), r.discard)?;
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
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owner {
    generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ref {
    generation: u64,
    draft_id: Uuid,
}
#[tauri::command]
pub async fn drafts_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<DraftInfo>> {
    let r: Owner = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .drafts(r.generation)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn draft_remove(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let r: Ref = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .remove_draft(r.generation, r.draft_id)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Restore {
    generation: u64,
    session_id: Uuid,
    draft_id: Uuid,
}
#[tauri::command]
pub async fn draft_restore(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<files::Document> {
    let r: Restore = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    state.authorize(r.generation)?;
    let read_state = state.inner().clone();
    let draft = tauri::async_runtime::spawn_blocking(move || {
        read_state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .draft(r.generation, r.draft_id)
    })
    .await
    .map_err(|_| AppError::io())??;
    {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        let session = sessions.authorize(r.session_id, r.generation)?;
        if !draft.identity.matches(&session.info.profile) {
            return Err(AppError::new(
                "WRONG_SERVER",
                "Conecte o perfil e endereço originais para recuperar este rascunho.",
            ));
        }
    }
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let result = files::documents::restore(&ctx, draft).await?;
    files::authorize(&state, r.session_id, r.generation)?;
    Ok(result)
}

#[derive(serde::Serialize)]
pub struct Preview {
    pub text: String,
}
#[tauri::command]
pub async fn draft_read(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Preview> {
    let r: Ref = serde_json::from_value(request).map_err(|_| AppError::invalid())?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let draft = state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .draft(r.generation, r.draft_id)?;
        Ok(Preview {
            text: draft.text.clone(),
        })
    })
    .await
    .map_err(|_| AppError::io())?
}
