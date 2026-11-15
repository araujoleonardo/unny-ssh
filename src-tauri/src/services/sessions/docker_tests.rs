use super::tests::{profile, state, wait_for};
use super::*;
use crate::{
    adapters::remote,
    services::{
        docker::{self, Action, Kind, projects, stream},
        files,
        profiles::{AuthMode, Secret, SecretPolicy},
        vault::drafts::Identity,
    },
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
fn fixture() -> (ProfileInput, Secret, KnownHost) {
    let dir = std::path::PathBuf::from(
        std::env::var_os("UNNY_DOCKER_FIXTURE_DIR").expect("disposable VM fixture required"),
    );
    let mut p = profile(
        std::env::var("UNNY_DOCKER_FIXTURE_PORT")
            .unwrap_or("22229".into())
            .parse()
            .unwrap(),
        AuthMode::Key,
    );
    p.host = std::env::var("UNNY_DOCKER_FIXTURE_HOST").unwrap();
    p.username = "unnyfixture".into();
    p.initial_directory = Some("/home/unnyfixture".into());
    let key = russh::keys::PublicKey::from_openssh(
        &std::fs::read_to_string(dir.join("host.pub")).unwrap(),
    )
    .unwrap();
    let known = KnownHost {
        host: p.host.clone(),
        port: p.port,
        key: key.to_openssh().unwrap(),
        fingerprint: key.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
    };
    (
        p,
        Secret::Key {
            private_key: std::fs::read_to_string(dir.join("id_ed25519")).unwrap(),
            passphrase: String::new(),
        },
        known,
    )
}
fn events(
    state: Arc<AppState>,
) -> (
    tauri::ipc::Channel<stream::Event>,
    Arc<Mutex<Vec<stream::Event>>>,
) {
    let output = Arc::new(Mutex::new(Vec::new()));
    let out = output.clone();
    let channel = tauri::ipc::Channel::new(move |message| {
        let tauri::ipc::InvokeResponseBody::Json(json) = message else {
            panic!("JSON event")
        };
        let event: stream::Event = serde_json::from_str(&json).unwrap();
        if event.kind == "stdout" || event.kind == "stderr" {
            let _ = stream::control(
                &state,
                event.session_id,
                event.generation,
                event.operation_id,
                Some(event.sequence),
            );
        }
        out.lock().unwrap().push(event);
        Ok(())
    });
    (channel, output)
}
async fn completed(events: &Mutex<Vec<stream::Event>>) -> String {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(e) = events
                .lock()
                .unwrap()
                .iter()
                .find(|e| ["completed", "failed", "cancelled"].contains(&e.kind.as_str()))
            {
                return e.kind.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn action(state: Arc<AppState>, session: Uuid, r: &docker::Resource, a: Action) {
    let token = docker::prepare(state.clone(), session, 1, r.handle, a)
        .await
        .unwrap();
    let result = docker::apply(state, session, 1, token.token, None)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
}
async fn compose(
    state: Arc<AppState>,
    session: Uuid,
    project: &projects::Project,
    a: projects::Action,
    volumes: bool,
) {
    let confirmation = projects::prepare(state.clone(), session, 1, project.id, a, volumes)
        .await
        .unwrap();
    let (channel, out) = events(state.clone());
    projects::apply(
        state,
        session,
        1,
        confirmation.token,
        (a == projects::Action::Down).then(|| project.name.clone()),
        confirmation.volume_token,
        volumes.then(|| format!("REMOVER VOLUMES {}", project.name)),
        channel,
    )
    .await
    .unwrap();
    assert_eq!(completed(&out).await, "completed");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires dedicated disposable Docker VM fixture"]
async fn native_docker_compose_lifecycle() {
    let (_dir, state) = state();
    let (mut p, secret, known) = fixture();
    p.id = None;
    let saved = state
        .vault
        .lock()
        .unwrap()
        .put_profile(1, p, SecretPolicy::Forget, None)
        .unwrap();
    let p = saved.fields;
    let session = start(
        state.clone(),
        1,
        p.clone(),
        Some(secret.clone()),
        Some(known.clone()),
    )
    .unwrap();
    wait_for(&state, session.id, "connected").await;
    let inventory = docker::inventory(state.clone(), session.id, 1, Kind::Container)
        .await
        .unwrap();
    let container = inventory
        .resources
        .iter()
        .find(|r| r.name == "unny-container-test")
        .unwrap()
        .clone();
    assert_eq!(container.state, "created");
    let inspection = docker::inspect(state.clone(), session.id, 1, &container)
        .await
        .unwrap();
    assert_eq!(inspection["Id"], container.id);
    for a in [
        Action::Start,
        Action::Pause,
        Action::Unpause,
        Action::Restart,
        Action::Stop,
        Action::Start,
    ] {
        action(state.clone(), session.id, &container, a).await;
    }
    let pending = docker::prepare(state.clone(), session.id, 1, container.handle, Action::Stop)
        .await
        .unwrap();
    remote::collect(
        state.clone(),
        session.id,
        1,
        remote::docker(&["container", "pause", "--", &container.id], None).unwrap(),
    )
    .await
    .unwrap()
    .success()
    .unwrap();
    assert_eq!(
        docker::apply(state.clone(), session.id, 1, pending.token, None)
            .await
            .err()
            .unwrap()
            .code,
        "STALE_RESOURCE"
    );
    action(state.clone(), session.id, &container, Action::Unpause).await;
    let volumes = docker::inventory(state.clone(), session.id, 1, Kind::Volume)
        .await
        .unwrap();
    let empty = volumes
        .resources
        .iter()
        .find(|v| v.name == "unny-empty")
        .unwrap();
    let used = volumes
        .resources
        .iter()
        .find(|v| v.name == "unny-in-use")
        .unwrap();
    let token = docker::prepare(state.clone(), session.id, 1, empty.handle, Action::Remove)
        .await
        .unwrap();
    assert_eq!(
        docker::apply(
            state.clone(),
            session.id,
            1,
            token.token,
            Some("wrong".into())
        )
        .await
        .err()
        .unwrap()
        .code,
        "NAME_REQUIRED"
    );
    let token = docker::prepare(state.clone(), session.id, 1, used.handle, Action::Remove)
        .await
        .unwrap();
    assert_ne!(
        docker::apply(
            state.clone(),
            session.id,
            1,
            token.token,
            Some(used.name.clone())
        )
        .await
        .unwrap()
        .exit_code,
        0
    );
    let token = docker::prepare(state.clone(), session.id, 1, empty.handle, Action::Remove)
        .await
        .unwrap();
    assert_eq!(
        docker::apply(
            state.clone(),
            session.id,
            1,
            token.token,
            Some(empty.name.clone())
        )
        .await
        .unwrap()
        .exit_code,
        0
    );
    let images = docker::inventory(state.clone(), session.id, 1, Kind::Image)
        .await
        .unwrap();
    let image = images
        .resources
        .iter()
        .find(|i| i.name == "busybox:1.37.0")
        .unwrap()
        .clone();
    let token = docker::prepare(state.clone(), session.id, 1, image.handle, Action::Remove)
        .await
        .unwrap();
    assert_ne!(
        docker::apply(state.clone(), session.id, 1, token.token, None)
            .await
            .unwrap()
            .exit_code,
        0
    );
    let (channel, out) = events(state.clone());
    let log = stream::start(
        state.clone(),
        session.id,
        1,
        remote::docker(
            &[
                "container",
                "logs",
                "--follow",
                "--tail",
                "200",
                "--",
                &container.id,
            ],
            None,
        )
        .unwrap(),
        channel,
        true,
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while out.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    stream::control(&state, session.id, 1, log, None).unwrap();
    assert_eq!(completed(&out).await, "cancelled");
    // PTY starts directly inside the container; no host initial-directory command is injected.
    let terminal_out = Arc::new(Mutex::new(Vec::new()));
    let copy = terminal_out.clone();
    let ack_state = state.clone();
    let channel = tauri::ipc::Channel::new(move |message| {
        let tauri::ipc::InvokeResponseBody::Json(json) = message else {
            panic!("JSON event")
        };
        let event: terminal::TerminalEvent = serde_json::from_str(&json).unwrap();
        if event.kind == "data" {
            copy.lock().unwrap().extend(&event.bytes);
            if let Some(t) = ack_state
                .sessions
                .lock()
                .unwrap()
                .terminals
                .get(&event.terminal_id)
            {
                let _ =
                    t.tx.try_send(terminal::TerminalCommand::Ack(event.sequence));
            }
        }
        Ok(())
    });
    let (reply, wait) = tokio::sync::oneshot::channel();
    let tx = state
        .sessions
        .lock()
        .unwrap()
        .authorize(session.id, 1)
        .unwrap()
        .tx
        .clone();
    tx.send(SessionCommand::OpenTerminal {
        request: terminal::OpenTerminal {
            command: Some(
                remote::docker(
                    &["container", "exec", "-it", "--", &container.id, "/bin/sh"],
                    None,
                )
                .unwrap()
                .into_bytes(),
            ),
            cols: 80,
            rows: 24,
            channel,
        },
        reply,
    })
    .await
    .unwrap();
    let terminal = wait.await.unwrap().unwrap();
    let terminal_tx = state
        .sessions
        .lock()
        .unwrap()
        .terminals
        .get(&terminal)
        .unwrap()
        .tx
        .clone();
    terminal_tx
        .send(terminal::TerminalCommand::Input(
            b"printf 'unny-exec-ready\\n'\rexit\r".to_vec(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !String::from_utf8_lossy(&terminal_out.lock().unwrap()).contains("unny-exec-ready") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let project = projects::validate(
        state.clone(),
        session.id,
        1,
        "/home/unnyfixture/compose's project".into(),
    )
    .await
    .unwrap();
    let project = state.vault.lock().unwrap().put_project(1, project).unwrap();
    assert_eq!(project.name, "unnyfixture");
    compose(
        state.clone(),
        session.id,
        &project,
        projects::Action::Up,
        false,
    )
    .await;
    let rows = projects::status(state.clone(), session.id, 1, &project)
        .await
        .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let worker = rows[0]["ID"].as_str().unwrap();
    remote::collect(
        state.clone(),
        session.id,
        1,
        remote::docker(
            &[
                "container",
                "exec",
                worker,
                "sh",
                "-c",
                "printf 'retained fixture data' > /data/retained",
            ],
            None,
        )
        .unwrap(),
    )
    .await
    .unwrap()
    .success()
    .unwrap();
    for a in [
        projects::Action::Stop,
        projects::Action::Start,
        projects::Action::Restart,
    ] {
        compose(state.clone(), session.id, &project, a, false).await;
    }
    let confirmation = projects::prepare(
        state.clone(),
        session.id,
        1,
        project.id,
        projects::Action::Down,
        true,
    )
    .await
    .unwrap();
    let (channel, _out) = events(state.clone());
    assert_eq!(
        projects::apply(
            state.clone(),
            session.id,
            1,
            confirmation.token,
            Some(project.name.clone()),
            None,
            None,
            channel
        )
        .await
        .err()
        .unwrap()
        .code,
        "VOLUME_CONFIRMATION_REQUIRED"
    );
    compose(
        state.clone(),
        session.id,
        &project,
        projects::Action::Down,
        false,
    )
    .await;
    let check = remote::collect(
        state.clone(),
        session.id,
        1,
        remote::docker(
            &[
                "run",
                "--rm",
                "--mount",
                "type=volume,source=unnyfixture_data,target=/data",
                "busybox:1.37.0",
                "cat",
                "/data/retained",
            ],
            None,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(check.exit_code, 0);
    assert_eq!(check.stdout, "retained fixture data");
    compose(
        state.clone(),
        session.id,
        &project,
        projects::Action::Up,
        false,
    )
    .await;
    compose(
        state.clone(),
        session.id,
        &project,
        projects::Action::Down,
        true,
    )
    .await;
    assert_ne!(
        remote::collect(
            state.clone(),
            session.id,
            1,
            remote::docker(&["volume", "inspect", "--", "unnyfixture_data"], None).unwrap()
        )
        .await
        .unwrap()
        .exit_code,
        0
    );
    action(state.clone(), session.id, &container, Action::Stop).await;
    action(state.clone(), session.id, &container, Action::Remove).await;
    let token = docker::prepare(state.clone(), session.id, 1, image.handle, Action::Remove)
        .await
        .unwrap();
    assert_eq!(
        docker::apply(state.clone(), session.id, 1, token.token, None)
            .await
            .unwrap()
            .exit_code,
        0
    );
    // A real second SSH user has no Docker socket permission. SFTP remains available.
    let mut denied = p.clone();
    denied.username = "unnynodocker".into();
    denied.initial_directory = Some("/home/unnynodocker".into());
    let other = start(
        state.clone(),
        1,
        denied,
        Some(secret.clone()),
        Some(known.clone()),
    )
    .unwrap();
    wait_for(&state, other.id, "connected").await;
    assert_eq!(
        docker::inventory(state.clone(), other.id, 1, Kind::Container)
            .await
            .err()
            .unwrap()
            .code,
        "PERMISSION_DENIED"
    );
    assert!(files::context(state.clone(), other.id, 1).await.is_ok());
    // Protect an actual dirty SFTP buffer together with the persisted Compose registration.
    let ctx = files::context(state.clone(), session.id, 1).await.unwrap();
    let listing = files::list(&ctx, None, None).await.unwrap();
    let note = listing
        .entries
        .iter()
        .find(|e| e.name == "note.txt")
        .unwrap();
    let files::OpenReply::Document { document } =
        files::open_document(&ctx, note.id).await.unwrap()
    else {
        panic!("text document")
    };
    let draft = files::documents::capture(
        &ctx,
        document.id,
        "unsaved recovered fixture\n".into(),
        Identity::from_profile(&p).unwrap(),
    )
    .unwrap();
    let draft_id = draft.id;
    crate::commands::lifecycle::invalidate(&state).unwrap();
    assert_eq!(
        state
            .vault
            .lock()
            .unwrap()
            .lock_drafts(1, vec![draft])
            .unwrap()
            .state,
        "locked"
    );
    assert!(state.sessions.lock().unwrap().entries.is_empty());
    assert_eq!(state.access.load(std::sync::atomic::Ordering::SeqCst), 0);
    state
        .vault
        .lock()
        .unwrap()
        .unlock("disposable master fixture")
        .unwrap();
    state.access.store(2, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        state
            .vault
            .lock()
            .unwrap()
            .projects(2, &Identity::from_profile(&p).unwrap())
            .unwrap()[0]
            .id,
        project.id
    );
    let restored = start(state.clone(), 2, p, Some(secret), Some(known)).unwrap();
    wait_for(&state, restored.id, "connected").await;
    let draft = state.vault.lock().unwrap().draft(2, draft_id).unwrap();
    let ctx = files::context(state.clone(), restored.id, 2).await.unwrap();
    let recovered = files::documents::restore(&ctx, draft).await.unwrap();
    assert_eq!(recovered.text, "unsaved recovered fixture\n");
    let remote = remote::collect(
        state.clone(),
        restored.id,
        2,
        remote::command(&["cat", "/home/unnyfixture/note.txt"], None).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(remote.stdout, "remote original\n");
    state.sessions.lock().unwrap().clear();
}

#[test]
#[ignore = "seeds only the explicitly isolated Docker UI fixture"]
fn native_docker_seed() {
    use crate::{adapters::storage::Storage, services::vault::Vault};
    let path = std::path::PathBuf::from(
        std::env::var_os("UNNY_SMOKE_DATA_DIR").expect("isolated smoke directory required"),
    );
    let (mut p, secret, known) = fixture();
    p.id = None;
    p.name = "Docker descartável".into();
    let mut vault = Vault::new(Storage::open(path).unwrap());
    vault.create("unny disposable smoke fixture").unwrap();
    vault.trust_host(1, known).unwrap();
    vault
        .put_profile(1, p, SecretPolicy::Save, Some(secret))
        .unwrap();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires dedicated VM user with Docker absent from PATH"]
async fn native_docker_missing() {
    let (_dir, state) = state();
    let (mut p, key, known) = fixture();
    p.username = "unnymissing".into();
    p.initial_directory = Some("/home/unnymissing".into());
    let session = start(state.clone(), 1, p, Some(key), Some(known)).unwrap();
    wait_for(&state, session.id, "connected").await;
    assert_eq!(
        docker::inventory(state.clone(), session.id, 1, Kind::Container)
            .await
            .err()
            .unwrap()
            .code,
        "DOCKER_UNAVAILABLE"
    );
    assert!(files::context(state.clone(), session.id, 1).await.is_ok());
    state.sessions.lock().unwrap().clear();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires dedicated VM with Compose plugin temporarily absent"]
async fn native_compose_missing() {
    let (_dir, state) = state();
    let (p, key, known) = fixture();
    let session = start(state.clone(), 1, p, Some(key), Some(known)).unwrap();
    wait_for(&state, session.id, "connected").await;
    assert!(
        docker::inventory(state.clone(), session.id, 1, Kind::Container)
            .await
            .is_ok()
    );
    assert_eq!(
        projects::discover(state.clone(), session.id, 1)
            .await
            .err()
            .unwrap()
            .code,
        "COMPOSE_UNAVAILABLE"
    );
    state.sessions.lock().unwrap().clear();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires disposable VM with a running unny-container-test"]
async fn native_lock_cancels_channels() {
    let (_dir, state) = state();
    let (mut p, secret, known) = fixture();
    p.id = None;
    let p = state
        .vault
        .lock()
        .unwrap()
        .put_profile(1, p, SecretPolicy::Forget, None)
        .unwrap()
        .fields;
    let session = start(
        state.clone(),
        1,
        p.clone(),
        Some(secret.clone()),
        Some(known.clone()),
    )
    .unwrap();
    wait_for(&state, session.id, "connected").await;
    let ctx = files::context(state.clone(), session.id, 1).await.unwrap();
    let list = files::list(&ctx, None, None).await.unwrap();
    let entry = list.entries.iter().find(|e| e.name == "note.txt").unwrap();
    let files::OpenReply::Document { document } =
        files::open_document(&ctx, entry.id).await.unwrap()
    else {
        panic!("document")
    };
    let draft = files::documents::capture(
        &ctx,
        document.id,
        "coordinated fixture draft".into(),
        Identity::from_profile(&p).unwrap(),
    )
    .unwrap();
    let permits = ctx
        .transfer_slots
        .clone()
        .acquire_many_owned(2)
        .await
        .unwrap();
    let destination = tempfile::tempdir().unwrap();
    let transfer = crate::services::transfers::start_download(
        state.clone(),
        ctx.clone(),
        entry.id,
        crate::services::transfers::Transfers::prepare_destination(
            destination.path().join("never-completed.txt"),
        )
        .unwrap(),
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .unwrap();
    assert_eq!(transfer.state, "queued");
    let inventory = docker::inventory(state.clone(), session.id, 1, Kind::Container)
        .await
        .unwrap();
    let container = inventory
        .resources
        .iter()
        .find(|r| r.name == "unny-container-test")
        .unwrap();
    let (channel, output) = events(state.clone());
    stream::start(
        state.clone(),
        session.id,
        1,
        remote::docker(
            &[
                "container",
                "logs",
                "--follow",
                "--tail",
                "1",
                "--",
                &container.id,
            ],
            None,
        )
        .unwrap(),
        channel,
        true,
    )
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while output.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let terminal_state = state.clone();
    let terminal_channel = tauri::ipc::Channel::new(move |message| {
        let tauri::ipc::InvokeResponseBody::Json(json) = message else {
            panic!("JSON")
        };
        let event: terminal::TerminalEvent = serde_json::from_str(&json).unwrap();
        if event.kind == "data" {
            if let Some(entry) = terminal_state
                .sessions
                .lock()
                .unwrap()
                .terminals
                .get(&event.terminal_id)
            {
                let _ = entry
                    .tx
                    .try_send(terminal::TerminalCommand::Ack(event.sequence));
            }
        }
        Ok(())
    });
    let (reply, wait) = tokio::sync::oneshot::channel();
    let tx = state
        .sessions
        .lock()
        .unwrap()
        .authorize(session.id, 1)
        .unwrap()
        .tx
        .clone();
    tx.send(SessionCommand::OpenTerminal {
        request: terminal::OpenTerminal {
            command: Some(
                remote::command(&["sh", "-c", "printf 'pty-ready\\n'; sleep 600"], None)
                    .unwrap()
                    .into_bytes(),
            ),
            cols: 80,
            rows: 24,
            channel: terminal_channel,
        },
        reply,
    })
    .await
    .unwrap();
    wait.await.unwrap().unwrap();
    let command_state = state.clone();
    let command = tokio::spawn(async move {
        remote::collect(
            command_state,
            session.id,
            1,
            remote::command(&["sleep", "600"], None).unwrap(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while state.docker.lock().unwrap().collects.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let lock_state = state.clone();
    let status = tokio::task::spawn_blocking(move || {
        crate::commands::lifecycle::invalidate(&lock_state).unwrap();
        lock_state
            .vault
            .lock()
            .unwrap()
            .lock_drafts(1, vec![draft])
            .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(status.state, "locked");
    drop(permits);
    assert!(command.await.unwrap().is_err());
    assert!(ctx.cancel.is_cancelled());
    assert!(state.sessions.lock().unwrap().entries.is_empty());
    assert!(state.sessions.lock().unwrap().terminals.is_empty());
    assert!(state.docker.lock().unwrap().jobs.is_empty());
    assert!(state.docker.lock().unwrap().collects.is_empty());
    assert!(
        state
            .transfers
            .lock()
            .unwrap()
            .list(session.id, 1)
            .is_empty()
    );
    assert_eq!(completed(&output).await, "cancelled");
    assert!(!destination.path().join("never-completed.txt").exists());
    state
        .vault
        .lock()
        .unwrap()
        .unlock("disposable master fixture")
        .unwrap();
    state.access.store(2, std::sync::atomic::Ordering::SeqCst);
    let restored = start(state.clone(), 2, p, Some(secret), Some(known)).unwrap();
    wait_for(&state, restored.id, "connected").await;
    let pattern = format!(
        "^docker --host unix:///var/run/docker.sock container logs --follow --tail 1 -- {}",
        container.id
    );
    let remaining = remote::collect(
        state.clone(),
        restored.id,
        2,
        remote::command(&["pgrep", "-f", &pattern], None).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        remaining.exit_code, 1,
        "Log follower remained on the disposable server"
    );
    let sleeps = remote::collect(
        state.clone(),
        restored.id,
        2,
        remote::command(&["pgrep", "-f", "^sleep 600$"], None).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        sleeps.exit_code, 1,
        "PTY/exec child remained on the disposable server"
    );
    state.sessions.lock().unwrap().clear();
}
