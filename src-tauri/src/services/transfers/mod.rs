use crate::{
    adapters::sftp::{self, CHUNK, Wire},
    commands::AppState,
    ipc::{AppError, Result},
    services::{
        files::{self, Context, Stamp},
        sessions,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collision {
    pub token: Uuid,
    pub target: String,
    pub non_atomic: bool,
    pub local: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: Uuid,
    pub session_id: Uuid,
    pub generation: u64,
    pub kind: String,
    pub state: String,
    pub name: String,
    pub destination: String,
    pub processed: u64,
    pub total: Option<u64>,
    pub ignored: Vec<String>,
    pub ignored_count: u64,
    pub residues: Vec<String>,
    pub message: Option<String>,
    pub collision: Option<Collision>,
    pub sequence: u64,
}
impl Snapshot {
    pub fn active(&self) -> bool {
        matches!(
            self.state.as_str(),
            "queued" | "running" | "waiting_collision" | "preparing"
        )
    }
}
struct Operation {
    snapshot: Snapshot,
    cancel: CancellationToken,
    channel: Channel<Snapshot>,
    awaiting: Option<u64>,
    last: Instant,
    decision: Option<tokio::sync::oneshot::Sender<Decision>>,
}
#[derive(Deserialize, Debug)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Overwrite { allow_non_atomic: bool },
    Rename { name: String },
    Skip,
}
pub struct Source {
    pub file: File,
    pub name: Vec<u8>,
    pub size: u64,
    pub modified: Option<SystemTime>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct LocalStamp {
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    inode: u64,
}
pub struct Destination {
    pub path: PathBuf,
    pub observed: Option<LocalStamp>,
}
struct Selection<T> {
    generation: u64,
    created: Instant,
    value: T,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    pub id: Uuid,
    pub name: String,
    pub size: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedDestination {
    pub id: Uuid,
    pub name: String,
    pub exists: bool,
}
pub struct Transfers {
    slots: Arc<tokio::sync::Semaphore>,
    operations: HashMap<Uuid, Operation>,
    sources: HashMap<Uuid, Selection<Source>>,
    destinations: HashMap<Uuid, Selection<Destination>>,
}
impl Default for Transfers {
    fn default() -> Self {
        Self {
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            operations: HashMap::new(),
            sources: HashMap::new(),
            destinations: HashMap::new(),
        }
    }
}
impl Transfers {
    pub fn cancel_owner(&self, session: Option<Uuid>) {
        for operation in self.operations.values() {
            if session.is_none_or(|id| operation.snapshot.session_id == id) {
                operation.cancel.cancel();
            }
        }
    }

    pub fn clear(&mut self) {
        for operation in self.operations.values() {
            operation.cancel.cancel();
        }
        self.operations.clear();
        self.sources.clear();
        self.destinations.clear();
    }
    pub fn cleanup(&mut self) {
        self.sources
            .retain(|_, s| s.created.elapsed() < Duration::from_secs(300));
        self.destinations
            .retain(|_, s| s.created.elapsed() < Duration::from_secs(300));
    }
    pub fn open_sources(paths: Vec<PathBuf>) -> Result<Vec<Source>> {
        if paths.len() > 64 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Selecione até 64 arquivos por vez.",
            ));
        }
        let mut opened = Vec::new();
        for path in paths {
            let file = File::open(&path).map_err(|_| AppError::io())?;
            let metadata = file.metadata().map_err(|_| AppError::io())?;
            if !metadata.is_file() {
                return Err(AppError::new(
                    "UNSUPPORTED",
                    "Upload aceita arquivos regulares. Pastas devem ser empacotadas antes de enviar.",
                ));
            }
            let name = local_name(path.file_name().ok_or_else(AppError::invalid)?);
            files::component(&name)?;
            opened.push(Source {
                file,
                name,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
        Ok(opened)
    }
    pub fn selected(&mut self, generation: u64, opened: Vec<Source>) -> Result<Vec<Selected>> {
        self.cleanup();
        if self.sources.len() + opened.len() > 64 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Limite de seleções atingido.",
            ));
        }
        let mut result = Vec::new();
        for source in opened {
            let id = Uuid::new_v4();
            result.push(Selected {
                id,
                name: files::display(&source.name),
                size: source.size,
            });
            self.sources.insert(
                id,
                Selection {
                    generation,
                    created: Instant::now(),
                    value: source,
                },
            );
        }
        Ok(result)
    }
    pub fn prepare_destination(path: PathBuf) -> Result<Destination> {
        let file = path.file_name().ok_or_else(AppError::invalid)?.to_owned();
        let parent = path
            .parent()
            .ok_or_else(AppError::invalid)?
            .canonicalize()
            .map_err(|_| AppError::io())?;
        let path = parent.join(file);
        let observed = local_stamp(&path)?;
        Ok(Destination { path, observed })
    }
    pub fn destination(
        &mut self,
        generation: u64,
        destination: Destination,
    ) -> Result<SelectedDestination> {
        self.cleanup();
        if self.destinations.len() >= 64 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Limite de destinos selecionados atingido.",
            ));
        }
        let id = Uuid::new_v4();
        let reply = SelectedDestination {
            id,
            name: destination.path.to_string_lossy().to_string(),
            exists: destination.observed.is_some(),
        };
        self.destinations.insert(
            id,
            Selection {
                generation,
                created: Instant::now(),
                value: destination,
            },
        );
        Ok(reply)
    }
    pub fn source(&mut self, id: Uuid, generation: u64) -> Result<Source> {
        take_selection(&mut self.sources, id, generation)
    }
    pub fn take_destination(&mut self, id: Uuid, generation: u64) -> Result<Destination> {
        take_selection(&mut self.destinations, id, generation)
    }
    pub fn list(&self, session: Uuid, generation: u64) -> Vec<Snapshot> {
        self.operations
            .values()
            .filter(|o| o.snapshot.session_id == session && o.snapshot.generation == generation)
            .map(|o| o.snapshot.clone())
            .collect()
    }
    fn operation(&mut self, id: Uuid, session: Uuid, generation: u64) -> Result<&mut Operation> {
        let op = self.operations.get_mut(&id).ok_or_else(sessions::unknown)?;
        if op.snapshot.session_id != session || op.snapshot.generation != generation {
            return Err(sessions::expired());
        }
        Ok(op)
    }
    pub fn cancel(&mut self, id: Uuid, session: Uuid, generation: u64) -> Result<()> {
        self.operation(id, session, generation)?.cancel.cancel();
        Ok(())
    }
    pub fn ack(&mut self, id: Uuid, session: Uuid, generation: u64, sequence: u64) -> Result<()> {
        let op = self.operation(id, session, generation)?;
        if op.awaiting != Some(sequence) {
            if !op.snapshot.active() {
                return Ok(());
            }
            return Err(AppError::invalid());
        }
        op.awaiting = None;
        Ok(())
    }
    pub fn resolve(
        &mut self,
        id: Uuid,
        session: Uuid,
        generation: u64,
        token: Uuid,
        decision: Decision,
    ) -> Result<()> {
        let op = self.operation(id, session, generation)?;
        if op.snapshot.collision.as_ref().map(|c| c.token) != Some(token) {
            return Err(AppError::new(
                "CONFIRMATION_EXPIRED",
                "A decisão de colisão expirou.",
            ));
        }
        op.decision
            .take()
            .ok_or_else(AppError::invalid)?
            .send(decision)
            .map_err(|_| sftp::interrupted())
    }
    fn insert(
        &mut self,
        session: Uuid,
        generation: u64,
        kind: &str,
        name: String,
        destination: String,
        cancel: CancellationToken,
        channel: Channel<Snapshot>,
    ) -> Result<Snapshot> {
        if self.operations.len() >= 64 {
            let old = self
                .operations
                .iter()
                .find(|(_, o)| !o.snapshot.active())
                .map(|(id, _)| *id)
                .ok_or_else(AppError::invalid)?;
            self.operations.remove(&old);
        }
        let snapshot = Snapshot {
            id: Uuid::new_v4(),
            session_id: session,
            generation,
            kind: kind.into(),
            state: "queued".into(),
            name,
            destination,
            processed: 0,
            total: None,
            ignored: Vec::new(),
            ignored_count: 0,
            residues: Vec::new(),
            message: None,
            collision: None,
            sequence: 0,
        };
        self.operations.insert(
            snapshot.id,
            Operation {
                snapshot: snapshot.clone(),
                cancel,
                channel,
                awaiting: None,
                last: Instant::now() - Duration::from_secs(1),
                decision: None,
            },
        );
        Ok(snapshot)
    }
}
fn take_selection<T>(
    map: &mut HashMap<Uuid, Selection<T>>,
    id: Uuid,
    generation: u64,
) -> Result<T> {
    let entry = map.remove(&id).ok_or_else(|| {
        AppError::new(
            "SELECTION_EXPIRED",
            "Seleção expirada. Use o seletor novamente.",
        )
    })?;
    if entry.generation != generation || entry.created.elapsed() >= Duration::from_secs(300) {
        return Err(sessions::expired());
    }
    Ok(entry.value)
}
fn local_name(name: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        name.to_string_lossy().as_bytes().to_vec()
    }
}
fn local_stamp(path: &Path) -> Result<Option<LocalStamp>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(AppError::new(
                    "UNSUPPORTED",
                    "Escolha um destino de arquivo regular; links e diretórios não são substituídos.",
                ));
            }
            #[cfg(unix)]
            use std::os::unix::fs::MetadataExt;
            Ok(Some(LocalStamp {
                size: metadata.len(),
                modified: metadata.modified().ok(),
                #[cfg(unix)]
                inode: metadata.ino(),
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(AppError::io()),
    }
}
fn update(state: &AppState, id: Uuid, force: bool, change: impl FnOnce(&mut Snapshot)) {
    if let Ok(mut manager) = state.transfers.lock() {
        if let Some(op) = manager.operations.get_mut(&id) {
            change(&mut op.snapshot);
            if force || (op.awaiting.is_none() && op.last.elapsed() >= Duration::from_millis(100)) {
                op.snapshot.sequence += 1;
                let sequence = op.snapshot.sequence;
                if op.channel.send(op.snapshot.clone()).is_err() {
                    op.cancel.cancel();
                }
                op.awaiting = Some(sequence);
                op.last = Instant::now();
            }
        }
    }
}
async fn decision(
    state: &AppState,
    id: Uuid,
    target: String,
    non_atomic: bool,
    local: bool,
    cancel: &CancellationToken,
) -> Result<Decision> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let token = Uuid::new_v4();
    {
        let mut manager = state.transfers.lock().map_err(|_| AppError::io())?;
        let op = manager
            .operations
            .get_mut(&id)
            .ok_or_else(sftp::interrupted)?;
        op.decision = Some(send);
    }
    update(state, id, true, |s| {
        s.state = "waiting_collision".into();
        s.collision = Some(Collision {
            token,
            target,
            non_atomic,
            local,
        });
    });
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err(sftp::interrupted()),result=tokio::time::timeout(Duration::from_secs(300),receive)=>result.map_err(|_|AppError::new("CONFIRMATION_EXPIRED","A decisão de colisão expirou.")).and_then(|v|v.map_err(|_|sftp::interrupted()))};
    update(state, id, true, |s| {
        s.state = "running".into();
        s.collision = None;
    });
    result
}
fn finish(state: &AppState, id: Uuid, result: Result<bool>, cancel: &CancellationToken) {
    update(state, id, true, |s| {
        s.collision = None;
        match result {
            Ok(true) => {
                s.state = "completed".into();
                s.message = Some(if s.ignored_count > 0 {
                    format!("Concluído com {} item(ns) ignorado(s).", s.ignored_count)
                } else {
                    "Transferência concluída.".into()
                });
            }
            Ok(false) => {
                s.state = "skipped".into();
                s.message = Some("Item ignorado por escolha do usuário.".into());
            }
            Err(error) => {
                s.state = if cancel.is_cancelled() {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                s.message = Some(format!("{} ({})", error.message, error.code));
            }
        }
    });
}
fn check(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(sftp::interrupted())
    } else {
        Ok(())
    }
}
fn session_cancel(state: &AppState, session: Uuid, generation: u64) -> Result<CancellationToken> {
    files::authorize(state, session, generation)?;
    Ok(state
        .sessions
        .lock()
        .map_err(|_| AppError::io())?
        .authorize(session, generation)?
        .cancel
        .child_token())
}

pub fn start_upload(
    state: Arc<AppState>,
    ctx: Arc<Context>,
    source: Source,
    directory: Uuid,
    channel: Channel<Snapshot>,
) -> Result<Snapshot> {
    let cancel = session_cancel(&state, ctx.session, ctx.generation)?;
    let parent = ctx.path(directory)?;
    let name = files::display(&source.name);
    let snapshot = state.transfers.lock().map_err(|_| AppError::io())?.insert(
        ctx.session,
        ctx.generation,
        "upload",
        name,
        files::display(&parent),
        cancel.clone(),
        channel,
    )?;
    let id = snapshot.id;
    let slots = state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .slots
        .clone();
    tauri::async_runtime::spawn(async move {
        let permits = tokio::select! {biased;_=cancel.cancelled()=>Err(sftp::interrupted()),result=async {let local=ctx.transfer_slots.clone().acquire_owned().await.map_err(|_|sftp::interrupted())?;let global=slots.acquire_owned().await.map_err(|_|sftp::interrupted())?;Ok::<_,AppError>((local,global))}=>result};
        let result = match permits {
            Ok(_permits) => {
                update(&state, id, true, |s| s.state = "preparing".into());
                upload(state.clone(), ctx, source, parent, id, cancel.clone()).await
            }
            Err(e) => Err(e),
        };
        finish(&state, id, result, &cancel);
    });
    Ok(snapshot)
}
async fn upload(
    state: Arc<AppState>,
    ctx: Arc<Context>,
    mut source: Source,
    parent: Vec<u8>,
    id: Uuid,
    cancel: CancellationToken,
) -> Result<bool> {
    let mut wire = files::lane(&state, ctx.session, ctx.generation, cancel.clone()).await?;
    if files::kind(&wire.stat(&parent, false).await?) != "directory" {
        return Err(AppError::invalid());
    }
    let mut target = files::join(&parent, &source.name)?;
    let mut allow_non_atomic = false;
    let observed = loop {
        check(&cancel)?;
        let existing = wire.maybe_stat(&target).await?;
        if existing.is_none() {
            break None;
        }
        if files::kind(existing.as_ref().unwrap()) != "file" {
            return Err(AppError::new(
                "UNSUPPORTED",
                "O destino existente não é um arquivo regular. Renomeie a origem antes de enviar.",
            ));
        }
        match decision(
            &state,
            id,
            files::display(&target),
            !wire.atomic_rename,
            false,
            &cancel,
        )
        .await?
        {
            Decision::Skip => return Ok(false),
            Decision::Rename { name } => {
                target = files::join(&parent, name.as_bytes())?;
            }
            Decision::Overwrite {
                allow_non_atomic: allowed,
            } => {
                if !wire.atomic_rename && !allowed {
                    return Err(AppError::new(
                        "UNSUPPORTED",
                        "A gravação não atômica exige aceite explícito.",
                    ));
                }
                allow_non_atomic = allowed;
                break existing;
            }
        }
    };
    let temporary = files::join(&parent, format!(".unny-upload-{id}.part").as_bytes())?;
    let residue = files::display(&temporary);
    update(&state, id, true, |s| {
        s.state = "running".into();
        s.total = Some(source.size);
        s.destination = files::display(&target);
    });
    let result = async {
        let handle = wire.open(&temporary, 2 | 8 | 32, Some(0o600)).await?;
        let mut offset = 0u64;
        // Local disk reads run on the blocking pool and never hold application locks.
        let mut buffer = vec![0; CHUNK];
        loop {
            check(&cancel)?;
            let file = source.file;
            let read = tokio::task::spawn_blocking(move || {
                let mut file = file;
                let n = file.read(&mut buffer);
                (file, buffer, n)
            })
            .await
            .map_err(|_| AppError::io())?;
            source.file = read.0;
            buffer = read.1;
            let n = read.2.map_err(|_| AppError::io())?;
            if n == 0 {
                break;
            }
            wire.write(&handle, offset, &buffer[..n]).await?;
            offset += n as u64;
            update(&state, id, false, |s| s.processed = offset);
        }
        let metadata = source.file.metadata().map_err(|_| AppError::io())?;
        if offset != source.size
            || metadata.len() != source.size
            || metadata.modified().ok() != source.modified
        {
            return Err(AppError::new(
                "SOURCE_CHANGED",
                "O arquivo local mudou durante o envio. O destino final não foi publicado.",
            ));
        }
        wire.set_permissions(
            &handle,
            observed
                .as_ref()
                .and_then(|a| a.permissions)
                .unwrap_or(0o644),
        )
        .await?;
        wire.sync(&handle).await?;
        wire.close(&handle).await?;
        if wire.maybe_stat(&target).await?.as_ref().map(Stamp::from)
            != observed.as_ref().map(Stamp::from)
        {
            return Err(AppError::new(
                "CONFLICT",
                "O destino mudou durante o envio. Revise uma nova transferência.",
            ));
        }
        if observed.is_none() {
            wire.rename(&temporary, &target, false).await?;
        } else if wire.atomic_rename {
            wire.rename(&temporary, &target, true).await?;
        } else if allow_non_atomic {
            let from = wire.open(&temporary, 1, None).await?;
            let to = wire.open(&target, 2 | 16, None).await?;
            let mut offset = 0;
            loop {
                let bytes = wire.read(&from, offset).await?;
                if bytes.is_empty() {
                    break;
                }
                wire.write(&to, offset, &bytes).await?;
                offset += bytes.len() as u64;
            }
            wire.sync(&to).await?;
            wire.close(&from).await?;
            wire.close(&to).await?;
            wire.remove(&temporary, false).await?;
        }
        Ok::<_, AppError>(true)
    }
    .await;
    if result.is_err() {
        // A cancelled SFTP stream cannot be reused; cleanup uses a fresh session lane.
        let cleaned = tokio::time::timeout(Duration::from_secs(3), async {
            let parent_cancel = session_cancel(&state, ctx.session, ctx.generation)?;
            let mut cleanup =
                files::lane(&state, ctx.session, ctx.generation, parent_cancel).await?;
            match cleanup.remove(&temporary, false).await {
                Ok(()) => Ok(()),
                Err(e) if e.code == "NOT_FOUND" => Ok(()),
                Err(e) => Err(e),
            }
        })
        .await;
        if !matches!(cleaned, Ok(Ok(()))) {
            update(&state, id, true, |s| s.residues.push(residue));
        }
    }
    result
}

pub fn start_download(
    state: Arc<AppState>,
    ctx: Arc<Context>,
    entry_id: Uuid,
    destination: Destination,
    channel: Channel<Snapshot>,
) -> Result<Snapshot> {
    let cancel = session_cancel(&state, ctx.session, ctx.generation)?;
    let path = ctx.path(entry_id)?;
    let snapshot = state.transfers.lock().map_err(|_| AppError::io())?.insert(
        ctx.session,
        ctx.generation,
        "download",
        files::display(files::basename(&path)),
        destination.path.to_string_lossy().to_string(),
        cancel.clone(),
        channel,
    )?;
    let id = snapshot.id;
    let runtime = tokio::runtime::Handle::current();
    let slots = state
        .transfers
        .lock()
        .map_err(|_| AppError::io())?
        .slots
        .clone();
    tauri::async_runtime::spawn(async move {
        let permits = tokio::select! {biased;_=cancel.cancelled()=>Err(sftp::interrupted()),result=async {let local=ctx.transfer_slots.clone().acquire_owned().await.map_err(|_|sftp::interrupted())?;let global=slots.acquire_owned().await.map_err(|_|sftp::interrupted())?;Ok::<_,AppError>((local,global))}=>result};
        match permits {
            Ok(_permits) => {
                update(&state, id, true, |s| s.state = "preparing".into());
                let finish_state = state.clone();
                let finish_cancel = cancel.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    let result = download(&state, &ctx, path, destination, id, &cancel, &runtime);
                    finish(&state, id, result, &cancel);
                })
                .await
                .map_err(|_| {
                    finish(&finish_state, id, Err(AppError::io()), &finish_cancel);
                });
            }
            Err(error) => finish(&state, id, Err(error), &cancel),
        }
    });
    Ok(snapshot)
}
fn download(
    state: &AppState,
    ctx: &Context,
    path: Vec<u8>,
    destination: Destination,
    id: Uuid,
    cancel: &CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> Result<bool> {
    let mut final_path = destination.path;
    let expected;
    loop {
        check(cancel)?;
        let current = local_stamp(&final_path)?;
        if current.is_none() {
            expected = None;
            break;
        }
        match runtime.block_on(decision(
            state,
            id,
            final_path.to_string_lossy().to_string(),
            false,
            true,
            cancel,
        ))? {
            Decision::Skip => return Ok(false),
            Decision::Overwrite { .. } => {
                expected = current;
                break;
            }
            Decision::Rename { name } => {
                local_component(&name)?;
                final_path = final_path
                    .parent()
                    .ok_or_else(AppError::invalid)?
                    .join(name);
            }
        }
    }
    let partial = final_path.with_file_name(format!(
        ".{}.{}.part",
        final_path.file_name().unwrap_or_default().to_string_lossy(),
        id
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&partial).map_err(|_| AppError::io())?;
    update(state, id, true, |s| {
        s.state = "running".into();
        s.destination = final_path.to_string_lossy().to_string();
    });
    let result = (|| {
        let mut wire = runtime.block_on(files::lane(
            state,
            ctx.session,
            ctx.generation,
            cancel.clone(),
        ))?;
        let attrs = runtime.block_on(wire.stat(&path, false))?;
        match files::kind(&attrs) {
            "file" => {
                update(state, id, true, |s| s.total = attrs.size);
                copy_remote(&mut wire, &path, &mut file, state, id, cancel, runtime)?;
            }
            "directory" => {
                let mut zip = zip::ZipWriter::new(file);
                let mut names = HashSet::new();
                let mut count = 0;
                zip_directory(
                    &mut wire,
                    &path,
                    &zip_component(files::basename(&path))?,
                    &mut zip,
                    &mut names,
                    &mut count,
                    0,
                    state,
                    id,
                    cancel,
                    runtime,
                )?;
                file = zip.finish().map_err(|_| AppError::io())?;
            }
            _ => {
                return Err(AppError::new(
                    "UNSUPPORTED",
                    "Links simbólicos e arquivos especiais não são seguidos no download.",
                ));
            }
        }
        check(cancel)?;
        file.flush().map_err(|_| AppError::io())?;
        file.sync_all().map_err(|_| AppError::io())?;
        drop(file);
        if local_stamp(&final_path)? != expected {
            return Err(AppError::new(
                "CONFLICT",
                "O destino local mudou durante o download. O arquivo .part foi preservado.",
            ));
        }
        if expected.is_some() {
            atomicwrites::replace_atomic(&partial, &final_path).map_err(|_| AppError::io())?;
        } else {
            atomicwrites::move_atomic(&partial, &final_path).map_err(|_| AppError::io())?;
        }
        Ok(true)
    })();
    if result.is_err() {
        update(state, id, true, |s| {
            s.residues.push(partial.to_string_lossy().to_string())
        });
    }
    result
}
fn copy_remote(
    wire: &mut Wire,
    path: &[u8],
    output: &mut impl Write,
    state: &AppState,
    id: Uuid,
    cancel: &CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> Result<()> {
    check(cancel)?;
    let attrs = runtime.block_on(wire.stat(path, false))?;
    if files::kind(&attrs) != "file" {
        return Err(AppError::new(
            "UNSUPPORTED",
            "O item deixou de ser um arquivo regular. Links não são seguidos.",
        ));
    }
    let handle = runtime.block_on(wire.open(path, 1, None))?;
    let before = runtime.block_on(wire.fstat(&handle))?;
    if files::kind(&before) != "file" {
        return Err(AppError::invalid());
    }
    let mut offset = 0;
    loop {
        check(cancel)?;
        let bytes = runtime.block_on(wire.read(&handle, offset))?;
        if bytes.is_empty() {
            break;
        }
        output.write_all(&bytes).map_err(|_| AppError::io())?;
        offset += bytes.len() as u64;
        update(state, id, false, |s| s.processed += bytes.len() as u64);
    }
    let after = runtime.block_on(wire.fstat(&handle))?;
    runtime.block_on(wire.close(&handle))?;
    if Stamp::from(&before) != Stamp::from(&after) || after.size.is_some_and(|size| size != offset)
    {
        return Err(AppError::new(
            "SOURCE_CHANGED",
            "O arquivo remoto mudou durante o download. O resultado permanece .part.",
        ));
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn zip_directory(
    wire: &mut Wire,
    path: &[u8],
    relative: &str,
    zip: &mut zip::ZipWriter<File>,
    names: &mut HashSet<String>,
    count: &mut usize,
    depth: usize,
    state: &AppState,
    id: Uuid,
    cancel: &CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> Result<()> {
    check(cancel)?;
    if depth > 32 || *count >= 50000 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "O ZIP excede 32 níveis ou 50.000 entradas. Baixe subpastas separadamente.",
        ));
    }
    let attrs = runtime.block_on(wire.stat(path, false))?;
    if files::kind(&attrs) != "directory" {
        return Err(AppError::new(
            "CONFLICT",
            "Uma pasta mudou de tipo durante o ZIP.",
        ));
    }
    let resolved = runtime.block_on(wire.realpath(path))?;
    if resolved != path {
        return Err(AppError::new(
            "CONFLICT",
            "Uma pasta mudou de caminho durante o ZIP. Links não são seguidos.",
        ));
    }
    let directory = format!("{relative}/");
    validate_zip(&directory)?;
    if !names.insert(directory.clone()) {
        return Err(AppError::new(
            "DUPLICATE_ENTRY",
            "Nomes duplicados no ZIP foram recusados.",
        ));
    }
    *count += 1;
    zip.add_directory(
        &directory,
        zip::write::SimpleFileOptions::default()
            .unix_permissions(attrs.permissions.unwrap_or(0o755) & 0o7777),
    )
    .map_err(|_| AppError::io())?;
    let handle = runtime.block_on(wire.opendir(path))?;
    while let Some(entries) = runtime.block_on(wire.readdir(&handle))? {
        if entries.is_empty() {
            return Err(sftp::protocol());
        }
        for entry in entries {
            check(cancel)?;
            if entry.name == b"." || entry.name == b".." {
                continue;
            }
            let child = files::join(path, &entry.name)?;
            let attrs = runtime.block_on(wire.stat(&child, false))?;
            match files::kind(&attrs) {
                "symlink" | "other" => {
                    update(state, id, false, |s| {
                        s.ignored_count += 1;
                        if s.ignored.len() < 50 {
                            s.ignored.push(files::display(&child));
                        }
                    });
                }
                "directory" => {
                    let relative = format!("{relative}/{}", zip_component(&entry.name)?);
                    zip_directory(
                        wire,
                        &child,
                        &relative,
                        zip,
                        names,
                        count,
                        depth + 1,
                        state,
                        id,
                        cancel,
                        runtime,
                    )?;
                }
                _ => {
                    *count += 1;
                    if *count > 50000 {
                        return Err(AppError::new(
                            "LIMIT_REACHED",
                            "O ZIP excede 50.000 entradas.",
                        ));
                    }
                    let name = format!("{relative}/{}", zip_component(&entry.name)?);
                    validate_zip(&name)?;
                    if !names.insert(name.clone()) {
                        return Err(AppError::new(
                            "DUPLICATE_ENTRY",
                            "Nomes duplicados no ZIP foram recusados.",
                        ));
                    }
                    zip.start_file(
                        name,
                        zip::write::SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Deflated)
                            .large_file(true)
                            .unix_permissions(attrs.permissions.unwrap_or(0o644) & 0o7777),
                    )
                    .map_err(|_| AppError::io())?;
                    copy_remote(wire, &child, zip, state, id, cancel, runtime)?;
                }
            }
        }
    }
    runtime.block_on(wire.close(&handle))?;
    Ok(())
}
pub fn zip_component(bytes: &[u8]) -> Result<String> {
    files::component(bytes)?;
    let mut result = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '%' || "\\:*?\"<>|".contains(c) || c.is_control() {
                for b in c.to_string().bytes() {
                    result.push_str(&format!("%{b:02X}"));
                }
            } else {
                result.push(c);
            }
        }
        for b in chunk.invalid() {
            result.push_str(&format!("%{b:02X}"));
        }
    }
    if result.ends_with('.') || result.ends_with(' ') {
        let byte = result.pop().unwrap() as u8;
        result.push_str(&format!("%{byte:02X}"));
    }
    validate_zip(&result)?;
    Ok(result)
}
pub fn validate_zip(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 4096
        || name.starts_with('/')
        || name.contains(['\\', ':', '\0'])
        || name
            .trim_end_matches('/')
            .split('/')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(AppError::new(
            "INVALID_ZIP_PATH",
            "Nome inseguro no ZIP foi recusado.",
        ));
    }
    Ok(())
}
fn local_component(name: &str) -> Result<()> {
    files::component(name.as_bytes())?;
    if name.contains(['\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(AppError::new(
            "INVALID_PATH",
            "Use um nome de arquivo local sem separadores ou caracteres reservados.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zip_names_are_relative_unambiguous_and_byte_preserving() {
        for name in [
            "../file", "/file", "a/../b", "a//b", "a\\b", "C:/file", "./file", "",
        ] {
            assert!(validate_zip(name).is_err(), "{name}");
        }
        assert!(validate_zip("folder/space file.txt").is_ok());
        assert_eq!(zip_component(b"bad_\xff.txt").unwrap(), "bad_%FF.txt");
        assert_eq!(zip_component(b"bad_%FF.txt").unwrap(), "bad_%25FF.txt");
        assert!(zip_component(b"..").is_err());
        assert!(zip_component(b"a/b").is_err());
    }
    #[test]
    fn zip64_writer_records_zip64_even_for_small_streams() {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("safe.txt", SimpleFileOptions::default().large_file(true))
            .unwrap();
        writer.write_all(b"content").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        // ZIP64 extended information extra field: ID 0x0001, length 16.
        assert!(bytes.windows(4).any(|w| w == [1, 0, 16, 0]));
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name().unwrap(), "safe.txt");
    }
}
