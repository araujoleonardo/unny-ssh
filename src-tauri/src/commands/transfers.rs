use super::AppState;
use crate::{
    ipc::{AppError, Result},
    services::{
        files,
        transfers::{self, Decision, Selected, SelectedDestination, Snapshot, Transfers},
    },
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{State, ipc::Channel};
use uuid::Uuid;
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| AppError::invalid())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owner {
    session_id: Uuid,
    generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectDestination {
    session_id: Uuid,
    generation: u64,
    entry_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Upload {
    session_id: Uuid,
    generation: u64,
    directory_id: Uuid,
    selection_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Download {
    session_id: Uuid,
    generation: u64,
    entry_id: Uuid,
    destination_id: Uuid,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Cancel,
    Ack { sequence: u64 },
    Resolve { token: Uuid, decision: Decision },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Action {
    session_id: Uuid,
    generation: u64,
    operation_id: Uuid,
    command: Control,
}
#[tauri::command]
pub async fn transfer_select_files(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Selected>> {
    use tauri_plugin_dialog::DialogExt;
    let r: Owner = parse(request)?;
    files::authorize(&state, r.session_id, r.generation)?;
    let sources = tauri::async_runtime::spawn_blocking(move || {
        let paths = app
            .dialog()
            .file()
            .set_title("Selecionar arquivos para upload")
            .blocking_pick_files()
            .unwrap_or_default();
        let paths = paths
            .into_iter()
            .map(|p| p.into_path().map_err(|_| AppError::invalid()))
            .collect::<Result<Vec<_>>>()?;
        Transfers::open_sources(paths)
    })
    .await
    .map_err(|_| AppError::io())??;
    files::authorize(&state, r.session_id, r.generation)?;
    state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .selected(r.generation, sources)
}
#[tauri::command]
pub async fn transfer_select_destination(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<SelectedDestination>> {
    use tauri_plugin_dialog::DialogExt;
    let r: SelectDestination = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let path = ctx.path(r.entry_id)?;
    let attrs = ctx.wire.lock().await.stat(&path, false).await?;
    let mut name = transfers::zip_component(files::basename(&path))?;
    if files::kind(&attrs) == "directory" {
        name.push_str(".zip");
    }
    let destination = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Baixar arquivo remoto — fora da proteção do cofre")
            .set_file_name(name)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|_| AppError::invalid())?;
        Ok(Some(Transfers::prepare_destination(path)?))
    })
    .await
    .map_err(|_| AppError::io())??;
    let Some(destination) = destination else {
        return Ok(None);
    };
    files::authorize(&state, r.session_id, r.generation)?;
    Ok(Some(
        state
            .transfers
            .lock()
            .map_err(|_| AppError::io())?
            .destination(r.generation, destination)?,
    ))
}
#[tauri::command]
pub async fn transfer_upload(
    request: serde_json::Value,
    channel: Channel<Snapshot>,
    state: State<'_, Arc<AppState>>,
) -> Result<Snapshot> {
    let r: Upload = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let source = state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .source(r.selection_id, r.generation)?;
    transfers::start_upload(state.inner().clone(), ctx, source, r.directory_id, channel)
}
#[tauri::command]
pub async fn transfer_download(
    request: serde_json::Value,
    channel: Channel<Snapshot>,
    state: State<'_, Arc<AppState>>,
) -> Result<Snapshot> {
    let r: Download = parse(request)?;
    let ctx = files::context(state.inner().clone(), r.session_id, r.generation).await?;
    let destination = state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .take_destination(r.destination_id, r.generation)?;
    transfers::start_download(state.inner().clone(), ctx, r.entry_id, destination, channel)
}
#[tauri::command]
pub fn transfers_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<Snapshot>> {
    let r: Owner = parse(request)?;
    state.authorize(r.generation)?;
    state
        .sessions
        .lock()
        .map_err(|_| AppError::io())?
        .authorize(r.session_id, r.generation)?;
    Ok(state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .list(r.session_id, r.generation))
}
#[tauri::command]
pub fn transfer_action(request: serde_json::Value, state: State<'_, Arc<AppState>>) -> Result<()> {
    let r: Action = parse(request)?;
    state.authorize(r.generation)?;
    state
        .sessions
        .lock()
        .map_err(|_| AppError::io())?
        .authorize(r.session_id, r.generation)?;
    let mut manager = state.transfers.lock().map_err(|_| AppError::io())?;
    match r.command {
        Control::Cancel => manager.cancel(r.operation_id, r.session_id, r.generation),
        Control::Ack { sequence } => {
            manager.ack(r.operation_id, r.session_id, r.generation, sequence)
        }
        Control::Resolve { token, decision } => {
            manager.resolve(r.operation_id, r.session_id, r.generation, token, decision)
        }
    }
}
#[derive(Default)]
pub struct ExitApproval(pub std::sync::atomic::AtomicBool);
#[tauri::command]
pub fn finish_window_close(app: tauri::AppHandle) -> Result<()> {
    use tauri::Manager;
    app.state::<ExitApproval>()
        .0
        .store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}
