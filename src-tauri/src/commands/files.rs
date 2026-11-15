use super::AppState;
use crate::{
    ipc::{AppError, Result},
    services::files::{self, Listing, Mutation, MutationReply, OpenReply, Revision, SaveReply},
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::State;
use uuid::Uuid;
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| AppError::invalid())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct List {
    session_id: Uuid,
    generation: u64,
    directory: Option<Uuid>,
    cursor: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Mutate {
    session_id: Uuid,
    generation: u64,
    command: Mutation,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Open {
    session_id: Uuid,
    generation: u64,
    entry_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Save {
    session_id: Uuid,
    generation: u64,
    document_id: Uuid,
    expected_revision: Revision,
    text: String,
    token: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Close {
    session_id: Uuid,
    generation: u64,
    document_id: Option<Uuid>,
    cursor: Option<Uuid>,
}
#[tauri::command]
pub async fn files_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Listing> {
    let r: List = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let result = files::list(&ctx, r.directory, r.cursor).await?;
    files::authorize(&state, r.session_id, r.generation)?;
    Ok(result)
}
#[tauri::command]
pub async fn files_mutate(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<MutationReply> {
    let r: Mutate = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    files::mutate(&ctx, r.command).await
}
#[tauri::command]
pub async fn document_open(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenReply> {
    let r: Open = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let result = files::open_document(&ctx, r.entry_id).await?;
    files::authorize(&state, r.session_id, r.generation)?;
    Ok(result)
}
#[tauri::command]
pub async fn document_save(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveReply> {
    let r: Save = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    files::save_document(&ctx, r.document_id, r.expected_revision, r.text, r.token).await
}
#[tauri::command]
pub async fn files_release(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let r: Close = parse(request)?;
    state.authorize(r.generation)?;
    let ctx = state
        .files
        .lock()
        .map_err(|_| AppError::io())?
        .contexts
        .get(&r.session_id)
        .cloned();
    if let Some(ctx) = ctx {
        if ctx.generation != r.generation {
            return Err(AppError::invalid());
        }
        if let Some(id) = r.document_id {
            ctx.documents
                .lock()
                .map_err(|_| AppError::io())?
                .remove(&id);
        }
        if let Some(id) = r.cursor {
            files::release_cursor(&ctx, id).await?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn files_terminal(
    request: serde_json::Value,
    channel: tauri::ipc::Channel<crate::services::sessions::terminal::TerminalEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    use crate::services::sessions::{self, SessionCommand, terminal};
    let r: Open = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let path = ctx.path(r.entry_id)?;
    {
        let mut wire = ctx.wire.lock().await;
        if files::kind(&wire.stat(&path, false).await?) != "directory"
            || wire.realpath(&path).await? != path
        {
            return Err(AppError::invalid());
        }
    }
    let command = terminal::directory_command(&path)?;
    let (reply, wait) = tokio::sync::oneshot::channel();
    {
        files::authorize(&state, r.session_id, r.generation)?;
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        sessions
            .authorize(r.session_id, r.generation)?
            .tx
            .try_send(SessionCommand::OpenTerminal {
                request: terminal::OpenTerminal {
                    command: Some(command),
                    cols: 80,
                    rows: 24,
                    channel,
                },
                reply,
            })
            .map_err(|_| super::busy())?;
    }
    wait.await.map_err(|_| sessions::unknown())?
}
