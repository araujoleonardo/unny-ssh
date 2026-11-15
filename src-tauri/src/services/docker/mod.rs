use crate::{
    adapters::remote,
    commands::AppState,
    ipc::{AppError, Result},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;
pub mod projects;
pub mod stream;
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Container,
    Image,
    Volume,
}
impl Kind {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Image => "image",
            Self::Volume => "volume",
        }
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}
impl Action {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Remove => "rm",
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub handle: Uuid,
    pub kind: Kind,
    pub id: String,
    pub name: String,
    pub state: String,
    pub image: String,
    pub ports: String,
    pub tags: String,
    pub project: String,
    pub details: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub version: String,
    pub resources: Vec<Resource>,
}
pub struct Context {
    pub resources: Mutex<HashMap<Uuid, Resource>>,
    pub permits: Mutex<HashMap<Uuid, Permit>>,
    pub slots: Arc<tokio::sync::Semaphore>,
    pub mutation: tokio::sync::Mutex<()>,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            resources: Mutex::default(),
            permits: Mutex::default(),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
            mutation: tokio::sync::Mutex::new(()),
        }
    }
}
#[derive(Clone)]
pub enum PermitAction {
    Resource {
        resource: Resource,
        action: Action,
        revision: String,
    },
    Compose {
        project: projects::Project,
        action: projects::Action,
        revision: String,
        volumes: Option<Uuid>,
    },
}
pub struct Permit {
    created: Instant,
    pub action: PermitAction,
}
#[derive(Default)]
pub struct Docker {
    contexts: HashMap<Uuid, Arc<Context>>,
    pub jobs: HashMap<Uuid, stream::Job>,
    pub collects: HashMap<Uuid, (Uuid, tokio_util::sync::CancellationToken)>,
}
impl Docker {
    pub fn cancel(&self, session: Option<Uuid>) {
        for job in self.jobs.values() {
            if session.is_none_or(|id| id == job.session) {
                job.cancel.cancel();
            }
        }
        for (owner, cancel) in self.collects.values() {
            if session.is_none_or(|id| id == *owner) {
                cancel.cancel();
            }
        }
    }
    pub fn busy(&self, session: Option<Uuid>) -> bool {
        self.jobs
            .values()
            .any(|j| session.is_none_or(|id| j.session == id))
            || self
                .collects
                .values()
                .any(|(owner, _)| session.is_none_or(|id| *owner == id))
    }
    pub fn clear(&mut self) {
        for job in self.jobs.values() {
            job.cancel.cancel();
        }
        for (_, cancel) in self.collects.values() {
            cancel.cancel();
        }
        self.collects.clear();
        self.jobs.clear();
        self.contexts.clear();
    }
}
pub fn context(state: &AppState, session: Uuid, generation: u64) -> Result<Arc<Context>> {
    remote::owner(state, session, generation)?;
    let mut manager = state.docker.lock().map_err(|_| AppError::io())?;
    Ok(manager.contexts.entry(session).or_default().clone())
}
pub fn forget(state: &AppState, session: Uuid) {
    if let Ok(mut d) = state.docker.lock() {
        d.contexts.remove(&session);
        for job in d.jobs.values().filter(|j| j.session == session) {
            job.cancel.cancel();
        }
    }
}
pub fn hash(value: &Value) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| AppError::invalid())?)
    ))
}
pub fn token(ctx: &Context, action: PermitAction) -> Result<Uuid> {
    let mut permits = ctx.permits.lock().map_err(|_| AppError::io())?;
    permits.retain(|_, p| p.created.elapsed() < Duration::from_secs(120));
    if permits.len() >= 32 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "Aguarde confirmações pendentes expirarem.",
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
pub fn consume(ctx: &Context, id: Uuid) -> Result<PermitAction> {
    let p = ctx
        .permits
        .lock()
        .map_err(|_| AppError::io())?
        .remove(&id)
        .ok_or_else(stale)?;
    if p.created.elapsed() >= Duration::from_secs(120) {
        return Err(stale());
    }
    Ok(p.action)
}
pub fn stale() -> AppError {
    AppError::new(
        "STALE_RESOURCE",
        "O recurso mudou ou a confirmação expirou. Atualize e confirme novamente.",
    )
}
pub fn valid_id(kind: Kind, id: &str) -> Result<()> {
    let valid = match kind {
        Kind::Container => id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        Kind::Image => id
            .strip_prefix("sha256:")
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        Kind::Volume => {
            !id.is_empty()
                && id.len() <= 255
                && id.as_bytes()[0].is_ascii_alphanumeric()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        }
    };
    if valid {
        Ok(())
    } else {
        Err(AppError::new(
            "INVALID_RESOURCE",
            "O servidor retornou um identificador Docker inválido.",
        ))
    }
}
fn field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}
pub fn parse_inventory(kind: Kind, text: &str) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        if resources.len() >= 5000 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Inventário excede 5.000 entradas.",
            ));
        }
        let v: Value = serde_json::from_str(line).map_err(|_| {
            AppError::new(
                "INVALID_DOCKER_RESPONSE",
                "O servidor retornou inventário Docker inválido.",
            )
        })?;
        let id = field(&v, if kind == Kind::Volume { "Name" } else { "ID" });
        valid_id(kind, &id)?;
        let labels = field(&v, "Labels");
        let project = labels
            .split(',')
            .find_map(|l| l.strip_prefix("com.docker.compose.project="))
            .unwrap_or("")
            .to_owned();
        let name = if kind == Kind::Container {
            field(&v, "Names")
        } else if kind == Kind::Volume {
            id.clone()
        } else {
            format!("{}:{}", field(&v, "Repository"), field(&v, "Tag"))
        };
        resources.push(Resource {
            handle: Uuid::new_v4(),
            kind,
            id,
            name,
            state: field(&v, "State"),
            image: field(&v, "Image"),
            ports: field(&v, "Ports"),
            tags: if kind == Kind::Image {
                format!("{} · {}", field(&v, "Tag"), field(&v, "Digest"))
            } else {
                String::new()
            },
            project,
            details: if kind == Kind::Container {
                field(&v, "Status")
            } else if kind == Kind::Image {
                field(&v, "Size")
            } else {
                format!("{} · {}", field(&v, "Driver"), field(&v, "Scope"))
            },
        });
    }
    Ok(resources)
}
pub async fn inventory(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    kind: Kind,
) -> Result<Inventory> {
    let ctx = context(&state, session, generation)?;
    let version = remote::collect(
        state.clone(),
        session,
        generation,
        remote::docker(&["version", "--format", "{{json .Server.Version}}"], None)?,
    )
    .await?;
    version.success()?;
    let args = match kind {
        Kind::Container => vec![
            "container",
            "ls",
            "--all",
            "--no-trunc",
            "--format",
            "{{json .}}",
        ],
        Kind::Image => vec![
            "image",
            "ls",
            "--no-trunc",
            "--digests",
            "--format",
            "{{json .}}",
        ],
        Kind::Volume => vec!["volume", "ls", "--format", "{{json .}}"],
    };
    let output = remote::collect(
        state.clone(),
        session,
        generation,
        remote::docker(&args, None)?,
    )
    .await?;
    output.success()?;
    let mut resources = parse_inventory(kind, &output.stdout)?;
    remote::owner(&state, session, generation)?;
    let mut cache = ctx.resources.lock().map_err(|_| AppError::io())?;
    for item in &mut resources {
        if let Some(old) = cache.values().find(|r| r.kind == kind && r.id == item.id) {
            item.handle = old.handle;
        }
    }
    cache.retain(|_, r| r.kind != kind);
    for r in &resources {
        cache.insert(r.handle, r.clone());
    }
    Ok(Inventory {
        version: serde_json::from_str::<String>(&version.stdout)
            .unwrap_or_else(|_| version.stdout.trim().to_owned()),
        resources,
    })
}
pub fn resource(ctx: &Context, id: Uuid) -> Result<Resource> {
    ctx.resources
        .lock()
        .map_err(|_| AppError::io())?
        .get(&id)
        .cloned()
        .ok_or_else(stale)
}
pub async fn inspect(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    resource: &Resource,
) -> Result<Value> {
    valid_id(resource.kind, &resource.id)?;
    let output = remote::collect(
        state,
        session,
        generation,
        remote::docker(&[resource.kind.cli(), "inspect", "--", &resource.id], None)?,
    )
    .await?;
    output.success()?;
    let values: Value = serde_json::from_str(&output.stdout)
        .map_err(|_| AppError::new("INVALID_DOCKER_RESPONSE", "Inspeção Docker inválida."))?;
    let array = values.as_array().ok_or_else(stale)?;
    if array.len() != 1 {
        return Err(stale());
    }
    Ok(array[0].clone())
}
pub fn allowed(kind: Kind, action: Action, state: &str) -> bool {
    if kind != Kind::Container {
        return action == Action::Remove;
    }
    match action {
        Action::Start => matches!(state, "created" | "exited" | "dead"),
        Action::Stop => matches!(state, "running" | "paused" | "restarting"),
        Action::Restart => matches!(state, "running" | "exited"),
        Action::Pause => state == "running",
        Action::Unpause => state == "paused",
        Action::Remove => matches!(state, "created" | "exited" | "dead"),
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub token: Uuid,
    pub server: String,
    pub name: String,
    pub action: String,
    pub needs_name: bool,
}
pub fn server(state: &AppState, session: Uuid, generation: u64) -> Result<String> {
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    let s = sessions.authorize(session, generation)?;
    Ok(format!(
        "{}@{}:{}",
        s.info.profile.username, s.info.profile.host, s.info.profile.port
    ))
}
pub async fn prepare(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    id: Uuid,
    action: Action,
) -> Result<Confirmation> {
    let ctx = context(&state, session, generation)?;
    let r = resource(&ctx, id)?;
    let value = inspect(state.clone(), session, generation, &r).await?;
    let status = value
        .pointer("/State/Status")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !allowed(r.kind, action, status) {
        return Err(stale());
    }
    let name = r.name.clone();
    let needs_name = r.kind == Kind::Volume;
    let token = token(
        &ctx,
        PermitAction::Resource {
            resource: r,
            action,
            revision: hash(&value)?,
        },
    )?;
    Ok(Confirmation {
        token,
        server: server(&state, session, generation)?,
        name,
        action: action.cli().into(),
        needs_name,
    })
}
pub async fn apply(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    id: Uuid,
    typed_name: Option<String>,
) -> Result<remote::Output> {
    let ctx = context(&state, session, generation)?;
    let cancel = remote::owner(&state, session, generation)?;
    let _guard = tokio::select! {_=cancel.cancelled()=>return Err(remote::interrupted()),g=ctx.mutation.lock()=>g};
    let PermitAction::Resource {
        resource: r,
        action,
        revision,
    } = consume(&ctx, id)?
    else {
        return Err(AppError::invalid());
    };
    if r.kind == Kind::Volume && typed_name.as_deref() != Some(&r.name) {
        return Err(AppError::new(
            "NAME_REQUIRED",
            "Digite exatamente o nome do volume para autorizar a perda de dados.",
        ));
    }
    let fresh = inspect(state.clone(), session, generation, &r).await?;
    if hash(&fresh)? != revision {
        return Err(stale());
    }
    remote::collect(
        state,
        session,
        generation,
        remote::docker(&[r.kind.cli(), action.cli(), "--", &r.id], None)?,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_and_allowlist_reject_shell_input() {
        assert!(valid_id(Kind::Container, &"a".repeat(64)).is_ok());
        for id in ["--all", "x;id", "a$(id)", "", "a\0b"] {
            assert!(valid_id(Kind::Container, id).is_err());
            assert!(valid_id(Kind::Image, id).is_err());
            assert!(valid_id(Kind::Volume, id).is_err());
        }
        assert!(!allowed(Kind::Image, Action::Start, ""));
        assert!(!allowed(Kind::Container, Action::Remove, "running"));
        assert!(allowed(Kind::Container, Action::Unpause, "paused"));
    }
    #[test]
    fn parses_inventory_without_treating_names_as_commands() {
        let id = "a".repeat(64);
        let rows=parse_inventory(Kind::Container,&serde_json::json!({"ID":id,"Names":"a'quoted;$(id)","State":"running","Labels":"com.docker.compose.project=fixture"}).to_string()).unwrap();
        assert_eq!(rows[0].name, "a'quoted;$(id)");
        assert_eq!(rows[0].project, "fixture");
        assert!(parse_inventory(Kind::Container, "not-json").is_err());
        assert!(parse_inventory(Kind::Image, r#"{"ID":"--all"}"#).is_err());
    }
    #[test]
    fn confirmations_expire_and_are_single_use() {
        let ctx = Context::default();
        let project = projects::Project {
            id: Uuid::new_v4(),
            identity: crate::services::vault::drafts::Identity {
                profile_id: Uuid::new_v4(),
                host: "fixture.invalid".into(),
                port: 22,
                username: "fixture".into(),
            },
            name: "fixture".into(),
            path: "/fixture".into(),
        };
        let action = PermitAction::Compose {
            project,
            action: projects::Action::Down,
            revision: "original".into(),
            volumes: None,
        };
        let id = token(&ctx, action.clone()).unwrap();
        assert!(consume(&Context::default(), id).is_err());
        assert!(consume(&ctx, id).is_ok());
        assert!(consume(&ctx, id).is_err());
        let id = token(&ctx, action).unwrap();
        ctx.permits.lock().unwrap().get_mut(&id).unwrap().created =
            Instant::now() - Duration::from_secs(121);
        assert!(consume(&ctx, id).is_err());
    }
}
