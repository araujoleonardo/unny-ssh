use super::*;
use crate::services::{vault::Vault, vault::drafts::Identity};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: Uuid,
    pub identity: Identity,
    pub name: String,
    pub path: String,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Up,
    Start,
    Stop,
    Restart,
    Down,
}
impl Action {
    pub fn cli(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Down => "down",
        }
    }
}
pub fn identity(state: &AppState, session: Uuid, generation: u64) -> Result<Identity> {
    remote::owner(state, session, generation)?;
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    Identity::from_profile(&sessions.authorize(session, generation)?.info.profile)
}
impl Vault {
    pub fn projects(&self, generation: u64, owner: &Identity) -> Result<Vec<Project>> {
        self.authorize(generation)?;
        self.payload()?
            .compose_projects
            .iter()
            .map(|v| serde_json::from_value::<Project>(v.clone()).map_err(|_| AppError::invalid()))
            .filter(|p| p.as_ref().map(|p| p.identity == *owner).unwrap_or(true))
            .collect()
    }
    pub fn put_project(&mut self, generation: u64, project: Project) -> Result<Project> {
        self.profile(generation, project.identity.profile_id)?;
        self.transaction(generation, |p| {
            if p.compose_projects.len() >= 128 {
                return Err(AppError::new(
                    "LIMIT_REACHED",
                    "Limite de 128 projetos registrados.",
                ));
            }
            if p.compose_projects.iter().any(|v| {
                serde_json::from_value::<Project>(v.clone())
                    .is_ok_and(|p| p.identity == project.identity && p.path == project.path)
            }) {
                return Err(AppError::new(
                    "ALREADY_EXISTS",
                    "Este diretório já está registrado neste perfil.",
                ));
            }
            p.compose_projects
                .push(serde_json::to_value(&project).map_err(|_| AppError::invalid())?);
            Ok(project)
        })
    }
    pub fn remove_project(&mut self, generation: u64, owner: &Identity, id: Uuid) -> Result<()> {
        self.transaction(generation, |p| {
            let index = p
                .compose_projects
                .iter()
                .position(|v| {
                    serde_json::from_value::<Project>(v.clone())
                        .is_ok_and(|project| project.id == id && project.identity == *owner)
                })
                .ok_or_else(AppError::invalid)?;
            p.compose_projects.remove(index);
            Ok(())
        })
    }
}
pub fn validate_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(AppError::new(
            "INVALID_PATH",
            "Informe um diretório remoto absoluto, sem caracteres de controle.",
        ));
    }
    Ok(())
}
pub fn project_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || !name.as_bytes()[0].is_ascii_lowercase() && !name.as_bytes()[0].is_ascii_digit()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
    {
        return Err(AppError::new(
            "INVALID_COMPOSE_PROJECT",
            "Compose retornou um nome de projeto inválido.",
        ));
    }
    Ok(())
}
pub fn command(project: &Project, args: &[&str]) -> Result<String> {
    validate_path(&project.path)?;
    project_name(&project.name)?;
    let mut all = vec![
        "compose",
        "--ansi",
        "never",
        "--project-directory",
        &project.path,
        "--project-name",
        &project.name,
    ];
    all.extend(args);
    remote::docker(&all, Some(&project.path))
}
pub fn action_command(project: &Project, action: Action, volumes: bool) -> Result<String> {
    if volumes && action != Action::Down {
        return Err(AppError::invalid());
    }
    let mut args = vec![action.cli()];
    if action == Action::Up {
        args.push("--detach");
    }
    if volumes {
        args.push("--volumes");
    }
    command(project, &args)
}
pub async fn discover(state: Arc<AppState>, session: Uuid, generation: u64) -> Result<String> {
    let output = remote::collect(
        state,
        session,
        generation,
        remote::docker(&["compose", "version", "--short"], None)?,
    )
    .await?;
    let version = output.stdout.trim().trim_start_matches('v');
    if output.exit_code != 0 || !version.starts_with("2.") {
        return Err(AppError::new(
            "COMPOSE_UNAVAILABLE",
            "Docker Compose v2 não está disponível neste servidor.",
        ));
    }
    Ok(version.to_owned())
}
pub async fn config(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    project: &Project,
) -> Result<Value> {
    let output = remote::collect(
        state,
        session,
        generation,
        command(project, &["config", "--format", "json"])?,
    )
    .await?;
    output.success()?;
    let config: Value = serde_json::from_str(&output.stdout).map_err(|_| {
        AppError::new(
            "INVALID_COMPOSE_CONFIG",
            "Compose retornou configuração inválida.",
        )
    })?;
    if !config
        .get("services")
        .and_then(Value::as_object)
        .is_some_and(|s| !s.is_empty() && s.len() <= 1000)
    {
        return Err(AppError::new(
            "INVALID_COMPOSE_CONFIG",
            "O projeto deve conter de 1 a 1.000 serviços.",
        ));
    }
    Ok(config)
}
pub async fn validate(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    path: String,
) -> Result<Project> {
    validate_path(&path)?;
    discover(state.clone(), session, generation).await?;
    let output = remote::collect(
        state.clone(),
        session,
        generation,
        format!("cd -- {} && pwd -P", remote::quote(&path)?),
    )
    .await?;
    output.success()?;
    let canonical = output.stdout.trim_end_matches('\n').to_owned();
    validate_path(&canonical)?;
    let output = remote::collect(
        state.clone(),
        session,
        generation,
        remote::docker(
            &["compose", "--ansi", "never", "config", "--format", "json"],
            Some(&canonical),
        )?,
    )
    .await?;
    output.success()?;
    let value: Value = serde_json::from_str(&output.stdout)
        .map_err(|_| AppError::new("INVALID_COMPOSE_CONFIG", "Configuração Compose inválida."))?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(AppError::invalid)?
        .to_owned();
    project_name(&name)?;
    if !value
        .get("services")
        .and_then(Value::as_object)
        .is_some_and(|s| !s.is_empty() && s.len() <= 1000)
    {
        return Err(AppError::new(
            "INVALID_COMPOSE_CONFIG",
            "A configuração não possui serviços válidos.",
        ));
    }
    Ok(Project {
        id: Uuid::new_v4(),
        identity: identity(&state, session, generation)?,
        name,
        path: canonical,
    })
}
pub fn get(state: &AppState, session: Uuid, generation: u64, id: Uuid) -> Result<Project> {
    let owner = identity(state, session, generation)?;
    state
        .vault
        .lock()
        .map_err(|_| AppError::io())?
        .projects(generation, &owner)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(stale)
}
pub async fn status(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    project: &Project,
) -> Result<Value> {
    discover(state.clone(), session, generation).await?;
    let output = remote::collect(
        state,
        session,
        generation,
        command(project, &["ps", "--all", "--format", "json"])?,
    )
    .await?;
    output.success()?;
    parse_ps(&output.stdout)
}
pub fn parse_ps(text: &str) -> Result<Value> {
    if text.trim().is_empty() {
        return Ok(Value::Array(vec![]));
    }
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        if value.is_array() {
            return Ok(value);
        }
        if value.is_object() {
            return Ok(Value::Array(vec![value]));
        }
    }
    let rows = text
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).map_err(|_| {
                AppError::new(
                    "INVALID_DOCKER_RESPONSE",
                    "Compose retornou estado inválido.",
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Value::Array(rows))
}
async fn revision(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    project: &Project,
) -> Result<String> {
    let config = config(state.clone(), session, generation, project).await?;
    let status = status(state, session, generation, project).await?;
    let mut stable: Vec<Value> = status
        .as_array()
        .ok_or_else(AppError::invalid)?
        .iter()
        .map(|row| {
            let mut fields = serde_json::Map::new();
            for key in [
                "ID",
                "Name",
                "Service",
                "Project",
                "State",
                "Health",
                "ExitCode",
                "Publishers",
            ] {
                if let Some(v) = row.get(key) {
                    fields.insert(key.to_owned(), v.clone());
                }
            }
            Value::Object(fields)
        })
        .collect();
    stable.sort_by_key(|r| r.get("ID").and_then(Value::as_str).unwrap_or("").to_owned());
    hash(&serde_json::json!({"config":config,"containers":stable}))
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirmation {
    pub token: Uuid,
    pub server: String,
    pub project: String,
    pub path: String,
    pub action: String,
    pub volume_token: Option<Uuid>,
}
pub async fn prepare(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    id: Uuid,
    action: Action,
    remove_volumes: bool,
) -> Result<Confirmation> {
    if remove_volumes && action != Action::Down {
        return Err(AppError::invalid());
    }
    let project = get(&state, session, generation, id)?;
    discover(state.clone(), session, generation).await?;
    let observed = revision(state.clone(), session, generation, &project).await?;
    let ctx = context(&state, session, generation)?;
    let volumes = remove_volumes.then(Uuid::new_v4);
    let token = token(
        &ctx,
        PermitAction::Compose {
            project: project.clone(),
            action,
            revision: observed,
            volumes,
        },
    )?;
    Ok(Confirmation {
        token,
        server: server(&state, session, generation)?,
        project: project.name,
        path: project.path,
        action: action.cli().into(),
        volume_token: volumes,
    })
}
pub async fn apply(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    token: Uuid,
    typed_project: Option<String>,
    volume_token: Option<Uuid>,
    volume_confirmation: Option<String>,
    channel: tauri::ipc::Channel<stream::Event>,
) -> Result<Uuid> {
    let ctx = context(&state, session, generation)?;
    let cancel = remote::owner(&state, session, generation)?;
    let _guard = tokio::select! {_=cancel.cancelled()=>return Err(remote::interrupted()),g=ctx.mutation.lock()=>g};
    let PermitAction::Compose {
        project,
        action,
        revision: expected,
        volumes,
    } = consume(&ctx, token)?
    else {
        return Err(AppError::invalid());
    };
    let current = get(&state, session, generation, project.id)?;
    if current.path != project.path || current.name != project.name {
        return Err(stale());
    }
    if action == Action::Down && typed_project.as_deref() != Some(&project.name) {
        return Err(AppError::new(
            "NAME_REQUIRED",
            "Digite exatamente o nome do projeto para autorizar down.",
        ));
    }
    if volumes != volume_token
        || volumes.is_some()
            && volume_confirmation.as_deref() != Some(&format!("REMOVER VOLUMES {}", project.name))
    {
        return Err(AppError::new(
            "VOLUME_CONFIRMATION_REQUIRED",
            "A remoção de volumes precisa de autorização adicional específica.",
        ));
    }
    if revision(state.clone(), session, generation, &project).await? != expected {
        return Err(stale());
    }
    stream::start(
        state,
        session,
        generation,
        action_command(&project, action, volumes.is_some())?,
        channel,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn down_preserves_volumes_unless_separately_requested() {
        let project = Project {
            id: Uuid::new_v4(),
            identity: Identity {
                profile_id: Uuid::new_v4(),
                host: "fixture.invalid".into(),
                port: 22,
                username: "fixture".into(),
            },
            name: "fixture".into(),
            path: "/srv/it's a project;$(id)".into(),
        };
        let cmd = action_command(&project, Action::Down, false).unwrap();
        assert!(!cmd.contains("--volumes"));
        assert!(cmd.contains("'/srv/it'\\''s a project;$(id)'"));
        assert!(
            action_command(&project, Action::Down, true)
                .unwrap()
                .contains("'--volumes'")
        );
        assert!(action_command(&project, Action::Up, true).is_err());
        assert!(validate_path("/tmp/x\0y").is_err());
        assert!(project_name("--option").is_err());
        assert!(
            parse_ps("{\"Name\":\"a\"}\n{\"Name\":\"b\"}")
                .unwrap()
                .as_array()
                .unwrap()
                .len()
                == 2
        );
    }
    #[test]
    fn projects_remain_bound_to_original_profile_and_address() {
        use crate::{
            adapters::storage::Storage,
            services::profiles::{AuthMode, ProfileInput, SecretPolicy},
        };
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
        vault.create("disposable master fixture").unwrap();
        let p = vault
            .put_profile(
                1,
                ProfileInput {
                    id: None,
                    name: "Fixture".into(),
                    host: "fixture.invalid".into(),
                    port: 22,
                    username: "fixture".into(),
                    auth_mode: AuthMode::Agent,
                    initial_directory: None,
                },
                SecretPolicy::Forget,
                None,
            )
            .unwrap();
        let owner = Identity::from_profile(&p.fields).unwrap();
        let project = Project {
            id: Uuid::new_v4(),
            identity: owner.clone(),
            name: "fixture".into(),
            path: "/srv/private project".into(),
        };
        let id = project.id;
        vault.put_project(1, project).unwrap();
        vault
            .change_password("disposable master fixture", "another disposable master")
            .unwrap();
        vault.lock();
        vault.unlock("another disposable master").unwrap();
        assert_eq!(vault.projects(3, &owner).unwrap()[0].id, id);
        let mut other = owner.clone();
        other.host = "another.invalid".into();
        assert!(vault.projects(3, &other).unwrap().is_empty());
        vault.remove_project(3, &owner, id).unwrap();
        assert!(vault.projects(3, &owner).unwrap().is_empty());
    }
}
