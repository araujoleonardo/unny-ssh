use super::AppState;
use crate::{
    adapters::remote,
    ipc::{AppError, Result},
    services::{
        docker::{self, Action, Kind, projects, stream},
        sessions::{SessionCommand, terminal},
    },
};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{State, ipc::Channel};
use uuid::Uuid;
fn parse<T: serde::de::DeserializeOwned>(r: serde_json::Value) -> Result<T> {
    serde_json::from_value(r).map_err(|_| AppError::invalid())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct List {
    session_id: Uuid,
    generation: u64,
    kind: Kind,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resource {
    session_id: Uuid,
    generation: u64,
    resource_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    session_id: Uuid,
    generation: u64,
    resource_id: Uuid,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Apply {
    session_id: Uuid,
    generation: u64,
    token: Uuid,
    typed_name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Control {
    session_id: Uuid,
    generation: u64,
    operation_id: Uuid,
    sequence: Option<u64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Owner {
    session_id: Uuid,
    generation: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Register {
    session_id: Uuid,
    generation: u64,
    path: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectRef {
    session_id: Uuid,
    generation: u64,
    project_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectPrepare {
    session_id: Uuid,
    generation: u64,
    project_id: Uuid,
    action: projects::Action,
    #[serde(default)]
    remove_volumes: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectApply {
    session_id: Uuid,
    generation: u64,
    token: Uuid,
    typed_project: Option<String>,
    volume_token: Option<Uuid>,
    volume_confirmation: Option<String>,
}
#[tauri::command]
pub async fn docker_inventory(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<docker::Inventory> {
    let r: List = parse(request)?;
    docker::inventory(state.inner().clone(), r.session_id, r.generation, r.kind).await
}
#[tauri::command]
pub async fn docker_inspect(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value> {
    let r: Resource = parse(request)?;
    let ctx = docker::context(&state, r.session_id, r.generation)?;
    let resource = docker::resource(&ctx, r.resource_id)?;
    let result =
        docker::inspect(state.inner().clone(), r.session_id, r.generation, &resource).await?;
    state.authorize(r.generation)?;
    Ok(result)
}
#[tauri::command]
pub async fn docker_prepare(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<docker::Confirmation> {
    let r: Prepare = parse(request)?;
    docker::prepare(
        state.inner().clone(),
        r.session_id,
        r.generation,
        r.resource_id,
        r.action,
    )
    .await
}
#[tauri::command]
pub async fn docker_apply(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<remote::Output> {
    let r: Apply = parse(request)?;
    docker::apply(
        state.inner().clone(),
        r.session_id,
        r.generation,
        r.token,
        r.typed_name,
    )
    .await
}
#[tauri::command]
pub async fn docker_logs(
    request: serde_json::Value,
    channel: Channel<stream::Event>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    let r: Resource = parse(request)?;
    let ctx = docker::context(&state, r.session_id, r.generation)?;
    let resource = docker::resource(&ctx, r.resource_id)?;
    if resource.kind != Kind::Container {
        return Err(AppError::invalid());
    }
    let _ = docker::inspect(state.inner().clone(), r.session_id, r.generation, &resource).await?;
    stream::start(
        state.inner().clone(),
        r.session_id,
        r.generation,
        remote::docker(
            &[
                "container",
                "logs",
                "--follow",
                "--timestamps",
                "--tail",
                "200",
                "--",
                &resource.id,
            ],
            None,
        )?,
        channel,
        true,
    )
}
#[tauri::command]
pub fn docker_stream_action(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let r: Control = parse(request)?;
    stream::control(
        &state,
        r.session_id,
        r.generation,
        r.operation_id,
        r.sequence,
    )
}
#[tauri::command]
pub async fn docker_exec(
    request: serde_json::Value,
    channel: Channel<terminal::TerminalEvent>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    let r: Resource = parse(request)?;
    let ctx = docker::context(&state, r.session_id, r.generation)?;
    let resource = docker::resource(&ctx, r.resource_id)?;
    if resource.kind != Kind::Container {
        return Err(AppError::invalid());
    }
    let inspection =
        docker::inspect(state.inner().clone(), r.session_id, r.generation, &resource).await?;
    if inspection
        .pointer("/State/Running")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
        || inspection
            .pointer("/State/Paused")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        return Err(docker::stale());
    }
    let cancel = remote::owner(&state, r.session_id, r.generation)?;
    let tx = {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        sessions.authorize(r.session_id, r.generation)?.tx.clone()
    };
    let (reply, wait) = tokio::sync::oneshot::channel();
    tx.try_send(SessionCommand::OpenTerminal {
        request: terminal::OpenTerminal {
            command: Some(
                remote::docker(
                    &[
                        "container",
                        "exec",
                        "--interactive",
                        "--tty",
                        "--",
                        &resource.id,
                        "/bin/sh",
                    ],
                    None,
                )?
                .into_bytes(),
            ),
            cols: 80,
            rows: 24,
            channel,
        },
        reply,
    })
    .map_err(|_| AppError::new("BUSY", "Aguarde o servidor processar as solicitações."))?;
    tokio::select! {_=cancel.cancelled()=>Err(remote::interrupted()),result=wait=>result.map_err(|_|remote::interrupted())?}
}
#[tauri::command]
pub async fn compose_list(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<projects::Project>> {
    let r: Owner = parse(request)?;
    let identity = projects::identity(&state, r.session_id, r.generation)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .projects(r.generation, &identity)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn compose_register(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<projects::Project> {
    let r: Register = parse(request)?;
    let state = state.inner().clone();
    let project = projects::validate(state.clone(), r.session_id, r.generation, r.path).await?;
    tauri::async_runtime::spawn_blocking(move || {
        remote::owner(&state, r.session_id, r.generation)?;
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .put_project(r.generation, project)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn compose_remove(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<()> {
    let r: ProjectRef = parse(request)?;
    let state = state.inner().clone();
    let identity = projects::identity(&state, r.session_id, r.generation)?;
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .remove_project(r.generation, &identity, r.project_id)
    })
    .await
    .map_err(|_| AppError::io())?
}
#[tauri::command]
pub async fn compose_status(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value> {
    let r: ProjectRef = parse(request)?;
    let project = projects::get(&state, r.session_id, r.generation, r.project_id)?;
    projects::status(state.inner().clone(), r.session_id, r.generation, &project).await
}
#[tauri::command]
pub async fn compose_prepare(
    request: serde_json::Value,
    state: State<'_, Arc<AppState>>,
) -> Result<projects::Confirmation> {
    let r: ProjectPrepare = parse(request)?;
    projects::prepare(
        state.inner().clone(),
        r.session_id,
        r.generation,
        r.project_id,
        r.action,
        r.remove_volumes,
    )
    .await
}
#[tauri::command]
pub async fn compose_apply(
    request: serde_json::Value,
    channel: Channel<stream::Event>,
    state: State<'_, Arc<AppState>>,
) -> Result<Uuid> {
    let r: ProjectApply = parse(request)?;
    projects::apply(
        state.inner().clone(),
        r.session_id,
        r.generation,
        r.token,
        r.typed_project,
        r.volume_token,
        r.volume_confirmation,
        channel,
    )
    .await
}
