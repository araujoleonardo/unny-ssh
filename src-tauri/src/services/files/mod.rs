use crate::{
    adapters::sftp::{Entry as WireEntry, Wire},
    commands::AppState,
    ipc::{AppError, Result},
    services::sessions::{self, SessionCommand},
};
use russh_sftp::protocol::FileAttributes;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub mod documents;
pub use documents::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub id: Uuid,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
    pub permissions: Option<u32>,
    pub mtime: Option<u32>,
    pub escaped: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub directory: RemoteEntry,
    pub parent: Option<RemoteEntry>,
    pub entries: Vec<RemoteEntry>,
    pub next_cursor: Option<Uuid>,
}
#[derive(Default)]
pub struct Files {
    pub contexts: HashMap<Uuid, Arc<Context>>,
}
pub struct Context {
    pub session: Uuid,
    pub generation: u64,
    pub cancel: CancellationToken,
    pub root: Vec<u8>,
    pub wire: tokio::sync::Mutex<Wire>,
    pub transfer_slots: Arc<tokio::sync::Semaphore>,
    paths: Mutex<Paths>,
    cursors: Mutex<HashMap<Uuid, Cursor>>,
    pub documents: Mutex<HashMap<Uuid, DocumentMeta>>,
    permits: Mutex<HashMap<Uuid, Permit>>,
}
#[derive(Default)]
struct Paths {
    by_id: HashMap<Uuid, Vec<u8>>,
    by_path: HashMap<Vec<u8>, Uuid>,
    bytes: usize,
}
struct Cursor {
    path: Vec<u8>,
    remote: Vec<u8>,
    pending: VecDeque<WireEntry>,
    created: Instant,
    done: bool,
}
struct Permit {
    created: Instant,
    action: PermitAction,
}
enum PermitAction {
    Mutation {
        path: Vec<u8>,
        observed: Stamp,
        target: Option<Vec<u8>>,
    },
    Save {
        document: Uuid,
        observed: Option<Revision>,
        fallback: bool,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub size: Option<u64>,
    pub mtime: Option<u32>,
    pub permissions: Option<u32>,
}
impl From<&FileAttributes> for Stamp {
    fn from(a: &FileAttributes) -> Self {
        Self {
            size: a.size,
            mtime: a.mtime,
            permissions: a.permissions,
        }
    }
}
pub fn kind(attrs: &FileAttributes) -> &'static str {
    match attrs.permissions.map(|p| p & 0xf000) {
        Some(0x4000) => "directory",
        Some(0x8000) => "file",
        Some(0xa000) => "symlink",
        _ => "other",
    }
}
pub fn display(bytes: &[u8]) -> String {
    let mut result = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c.is_control() {
                result.extend(c.escape_default());
            } else {
                result.push(c);
            }
        }
        for byte in chunk.invalid() {
            result.push_str(&format!("\\x{byte:02X}"));
        }
    }
    result
}
pub fn component(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty()
        || bytes == b"."
        || bytes == b".."
        || bytes.len() > 4096
        || bytes.contains(&b'/')
        || bytes.contains(&0)
    {
        return Err(AppError::new(
            "INVALID_PATH",
            "Use um nome simples, sem barras, NUL, ponto ou ponto duplo.",
        ));
    }
    Ok(())
}
pub fn join(parent: &[u8], name: &[u8]) -> Result<Vec<u8>> {
    component(name)?;
    let mut path = parent.to_vec();
    if !path.ends_with(b"/") {
        path.push(b'/');
    }
    path.extend(name);
    if path.len() > 16384 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "O caminho remoto excede o limite.",
        ));
    }
    Ok(path)
}
pub fn parent(path: &[u8]) -> Option<Vec<u8>> {
    if path == b"/" {
        return None;
    }
    let index = path.iter().rposition(|b| *b == b'/')?;
    Some(if index == 0 {
        b"/".to_vec()
    } else {
        path[..index].to_vec()
    })
}
pub fn basename(path: &[u8]) -> &[u8] {
    path.rsplit(|b| *b == b'/')
        .next()
        .filter(|v| !v.is_empty())
        .unwrap_or(b"root")
}
impl Context {
    pub fn path(&self, id: Uuid) -> Result<Vec<u8>> {
        self.paths
            .lock()
            .map_err(|_| AppError::io())?
            .by_id
            .get(&id)
            .cloned()
            .ok_or_else(sessions::unknown)
    }
    pub fn entry(&self, path: Vec<u8>, attrs: &FileAttributes) -> Result<RemoteEntry> {
        let mut paths = self.paths.lock().map_err(|_| AppError::io())?;
        let id = if let Some(id) = paths.by_path.get(&path) {
            *id
        } else {
            if paths.by_id.len() >= 20000 || paths.bytes + path.len() > 8 * 1024 * 1024 {
                return Err(AppError::new(
                    "LIMIT_REACHED",
                    "Limite de caminhos desta sessão atingido. Feche a sessão para liberar os handles.",
                ));
            }
            let id = Uuid::new_v4();
            paths.bytes += path.len();
            paths.by_path.insert(path.clone(), id);
            paths.by_id.insert(id, path.clone());
            id
        };
        Ok(RemoteEntry {
            id,
            name: if path == b"/" {
                "/".into()
            } else {
                display(basename(&path))
            },
            path: display(&path),
            kind: kind(attrs).into(),
            size: attrs.size,
            permissions: attrs.permissions.map(|p| p & 0o7777),
            mtime: attrs.mtime,
            escaped: std::str::from_utf8(&path).is_err(),
        })
    }
    fn permit(&self, action: PermitAction) -> Result<Uuid> {
        let mut permits = self.permits.lock().map_err(|_| AppError::io())?;
        permits.retain(|_, p| p.created.elapsed() < Duration::from_secs(120));
        if permits.len() >= 32 {
            return Err(AppError::new(
                "BUSY",
                "Limite de confirmações pendentes atingido.",
            ));
        }
        let id = Uuid::new_v4();
        permits.insert(
            id,
            Permit {
                created: Instant::now(),
                action,
            },
        );
        Ok(id)
    }
    fn consume(&self, id: Uuid) -> Result<PermitAction> {
        let permit = self
            .permits
            .lock()
            .map_err(|_| AppError::io())?
            .remove(&id)
            .ok_or_else(expired)?;
        if permit.created.elapsed() >= Duration::from_secs(120) {
            return Err(expired());
        }
        Ok(permit.action)
    }
}
fn expired() -> AppError {
    AppError::new(
        "CONFIRMATION_EXPIRED",
        "A confirmação expirou. Atualize o recurso e tente novamente.",
    )
}
pub async fn lane(
    state: &AppState,
    session: Uuid,
    generation: u64,
    cancel: CancellationToken,
) -> Result<Wire> {
    state.authorize(generation)?;
    let tx = {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        let entry = sessions.authorize(session, generation)?;
        if entry.info.state != "connected" || !entry.info.sftp {
            return Err(AppError::new(
                "SFTP_UNAVAILABLE",
                "SFTP indisponível nesta sessão. O terminal continua independente.",
            ));
        }
        entry.tx.clone()
    };
    let (reply, wait) = tokio::sync::oneshot::channel();
    tx.try_send(SessionCommand::OpenSftp { reply })
        .map_err(|_| {
            AppError::new(
                "BUSY",
                "Aguarde a sessão processar as solicitações anteriores.",
            )
        })?;
    let channel = tokio::select! {_=cancel.cancelled()=>return Err(crate::adapters::sftp::interrupted()),result=wait=>result.map_err(|_|sessions::unknown())??};
    Wire::new(channel.into_stream(), cancel).await
}
pub fn authorize(state: &AppState, session: Uuid, generation: u64) -> Result<()> {
    state.authorize(generation)?;
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    let entry = sessions.authorize(session, generation)?;
    if entry.info.state != "connected" || entry.cancel.is_cancelled() {
        return Err(AppError::new(
            "CONNECTION_LOST",
            "A sessão foi encerrada. Suas edições permanecem na interface.",
        ));
    }
    Ok(())
}
pub async fn context(state: Arc<AppState>, session: Uuid, generation: u64) -> Result<Arc<Context>> {
    authorize(&state, session, generation)?;
    if let Some(context) = state
        .files
        .lock()
        .map_err(|_| AppError::io())?
        .contexts
        .get(&session)
        .cloned()
    {
        return Ok(context);
    }
    let (cancel, initial) = {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        let entry = sessions.authorize(session, generation)?;
        (
            entry.cancel.child_token(),
            entry
                .info
                .profile
                .initial_directory
                .clone()
                .unwrap_or(".".into()),
        )
    };
    let mut wire = lane(&state, session, generation, cancel.clone()).await?;
    let root = wire.realpath(initial.as_bytes()).await?;
    let ctx = Arc::new(Context {
        session,
        generation,
        cancel,
        root,
        wire: tokio::sync::Mutex::new(wire),
        transfer_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        paths: Mutex::default(),
        cursors: Mutex::default(),
        documents: Mutex::default(),
        permits: Mutex::default(),
    });
    state.authorize(generation)?;
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    let entry = sessions.authorize(session, generation)?;
    if entry.cancel.is_cancelled() {
        return Err(sessions::expired());
    }
    let mut manager = state.files.lock().map_err(|_| AppError::io())?;
    Ok(manager.contexts.entry(session).or_insert(ctx).clone())
}
pub async fn list(ctx: &Context, directory: Option<Uuid>, cursor: Option<Uuid>) -> Result<Listing> {
    if ctx.cancel.is_cancelled() {
        return Err(sessions::expired());
    }
    let path = match directory {
        Some(id) => ctx.path(id)?,
        None => ctx.root.clone(),
    };
    let mut wire = ctx.wire.lock().await;
    let attrs = wire.stat(&path, false).await?;
    if kind(&attrs) != "directory" {
        return Err(AppError::new(
            "NOT_DIRECTORY",
            "Este item não é uma pasta. Links não são expandidos automaticamente.",
        ));
    }
    let current = ctx.entry(path.clone(), &attrs)?;
    let parent = if let Some(parent) = parent(&path) {
        let attrs = wire.stat(&parent, false).await?;
        Some(ctx.entry(parent, &attrs)?)
    } else {
        None
    };
    let mut page = if let Some(id) = cursor {
        let page = ctx
            .cursors
            .lock()
            .map_err(|_| AppError::io())?
            .remove(&id)
            .ok_or_else(expired)?;
        if page.path != path {
            let _ = wire.close(&page.remote).await;
            return Err(AppError::invalid());
        }
        if page.created.elapsed() > Duration::from_secs(60) {
            let _ = wire.close(&page.remote).await;
            return Err(expired());
        }
        page
    } else {
        let stale = {
            let mut cursors = ctx.cursors.lock().map_err(|_| AppError::io())?;
            let ids: Vec<_> = cursors
                .iter()
                .filter(|(_, p)| p.created.elapsed() > Duration::from_secs(60))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| cursors.remove(&id))
                .collect::<Vec<_>>()
        };
        for old in stale {
            let _ = wire.close(&old.remote).await;
        }
        if ctx.cursors.lock().map_err(|_| AppError::io())?.len() >= 8 {
            return Err(AppError::new(
                "BUSY",
                "Feche ou atualize uma listagem antes de abrir outra.",
            ));
        }
        Cursor {
            remote: wire.opendir(&path).await?,
            path: path.clone(),
            pending: VecDeque::new(),
            created: Instant::now(),
            done: false,
        }
    };
    let result = async {
        let mut entries = Vec::new();
        while entries.len() < 200 {
            if let Some(entry) = page.pending.pop_front() {
                if entry.name == b"." || entry.name == b".." {
                    continue;
                }
                let child = join(&path, &entry.name)?;
                entries.push(ctx.entry(child, &entry.attrs)?);
                continue;
            }
            if page.done {
                break;
            }
            match wire.readdir(&page.remote).await? {
                Some(entries) => {
                    if entries.is_empty() {
                        return Err(crate::adapters::sftp::protocol());
                    }
                    page.pending.extend(entries);
                }
                None => page.done = true,
            }
        }
        Ok::<_, AppError>(entries)
    }
    .await;
    let entries = match result {
        Ok(entries) => entries,
        Err(error) => {
            let _ = wire.close(&page.remote).await;
            return Err(error);
        }
    };
    let next_cursor = if page.done && page.pending.is_empty() {
        wire.close(&page.remote).await?;
        None
    } else {
        let token = Uuid::new_v4();
        ctx.cursors
            .lock()
            .map_err(|_| AppError::io())?
            .insert(token, page);
        Some(token)
    };
    Ok(Listing {
        directory: current,
        parent,
        entries,
        next_cursor,
    })
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Create {
        parent: Uuid,
        name: String,
        directory: bool,
    },
    Prepare {
        entry: Uuid,
        name: Option<String>,
    },
    Apply {
        token: Uuid,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MutationReply {
    Created {
        entry: RemoteEntry,
    },
    Confirmation {
        token: Uuid,
        entry: RemoteEntry,
        target: Option<String>,
    },
    Done,
}
pub async fn mutate(ctx: &Context, action: Mutation) -> Result<MutationReply> {
    if ctx.cancel.is_cancelled() {
        return Err(sessions::expired());
    }
    let mut wire = ctx.wire.lock().await;
    match action {
        Mutation::Create {
            parent,
            name,
            directory,
        } => {
            let parent = ctx.path(parent)?;
            if kind(&wire.stat(&parent, false).await?) != "directory" {
                return Err(AppError::invalid());
            }
            let path = join(&parent, name.as_bytes())?;
            if wire.maybe_stat(&path).await?.is_some() {
                return Err(AppError::new(
                    "ALREADY_EXISTS",
                    "Já existe um item com esse nome.",
                ));
            }
            if directory {
                wire.mkdir(&path).await?;
            } else {
                let handle = wire.open(&path, 2 | 8 | 32, Some(0o644)).await?;
                wire.close(&handle).await?;
            }
            let attrs = wire.stat(&path, false).await?;
            Ok(MutationReply::Created {
                entry: ctx.entry(path, &attrs)?,
            })
        }
        Mutation::Prepare { entry, name } => {
            let path = ctx.path(entry)?;
            if path == b"/" {
                return Err(AppError::new(
                    "PERMISSION_DENIED",
                    "A raiz não pode ser removida ou renomeada.",
                ));
            }
            let attrs = wire.stat(&path, false).await?;
            let target = if let Some(name) = name {
                let target = join(
                    &parent(&path).ok_or_else(AppError::invalid)?,
                    name.as_bytes(),
                )?;
                if wire.maybe_stat(&target).await?.is_some() {
                    return Err(AppError::new(
                        "ALREADY_EXISTS",
                        "O destino já existe. Nenhum item foi substituído.",
                    ));
                }
                Some(target)
            } else {
                None
            };
            let token = ctx.permit(PermitAction::Mutation {
                path: path.clone(),
                observed: Stamp::from(&attrs),
                target: target.clone(),
            })?;
            Ok(MutationReply::Confirmation {
                token,
                entry: ctx.entry(path, &attrs)?,
                target: target.as_deref().map(display),
            })
        }
        Mutation::Apply { token } => {
            let PermitAction::Mutation {
                path,
                observed,
                target,
            } = ctx.consume(token)?
            else {
                return Err(AppError::invalid());
            };
            let attrs = wire.stat(&path, false).await?;
            if Stamp::from(&attrs) != observed {
                return Err(AppError::new(
                    "CONFLICT",
                    "O item mudou desde a confirmação. Atualize e revise novamente.",
                ));
            }
            if let Some(target) = target {
                if wire.maybe_stat(&target).await?.is_some() {
                    return Err(AppError::new(
                        "ALREADY_EXISTS",
                        "O destino passou a existir. Nenhum item foi substituído.",
                    ));
                }
                wire.rename(&path, &target, false).await?;
            } else {
                wire.remove(&path, kind(&attrs) == "directory").await?;
            }
            Ok(MutationReply::Done)
        }
    }
}

pub async fn release_cursor(ctx: &Context, id: Uuid) -> Result<()> {
    let cursor = ctx.cursors.lock().map_err(|_| AppError::io())?.remove(&id);
    if let Some(cursor) = cursor {
        ctx.wire.lock().await.close(&cursor.remote).await?;
    }
    Ok(())
}
