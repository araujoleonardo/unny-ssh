use super::*;
use crate::{
    adapters::storage::Storage,
    services::{
        profiles::{AuthMode, SecretPolicy},
        vault::Vault,
    },
};
use russh::{
    ChannelId,
    keys::{Algorithm, PrivateKey, PublicKey, ssh_key::LineEnding},
    server,
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
struct Fixture {
    auth_attempts: Arc<AtomicUsize>,
    public: PublicKey,
    resizes: Arc<Mutex<Vec<(u32, u32)>>>,
}
impl server::Handler for Fixture {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.auth_attempts.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "fixture" && password == "synthetic-only" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: server::VerifiedPublicKey<'_>,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.auth_attempts.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "fixture" && key.public_key == &self.public {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        session.channel_failure(channel)?;
        Ok(())
    }
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.resizes.lock().unwrap().push((cols, rows));
        session.channel_success(channel)?;
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        session.channel_success(channel)?;
        session.data(channel, vec![0xc3])?;
        session.data(channel, vec![0xa9])?;
        Ok(())
    }
    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        cols: u32,
        rows: u32,
        _pw: u32,
        _ph: u32,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.resizes.lock().unwrap().push((cols, rows));
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if data == b"exit" {
            session.exit_status_request(channel, 0)?;
            session.close(channel)?;
        } else if data == b"flood" {
            for _ in 0..64 {
                session.data(channel, vec![b'x'; 16384])?;
            }
        } else {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }
}
struct ServerFixture {
    port: u16,
    key: PrivateKey,
    auth: Arc<AtomicUsize>,
    resizes: Arc<Mutex<Vec<(u32, u32)>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ServerFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(public: PublicKey) -> ServerFixture {
    let key = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Arc::new(server::Config {
        keys: vec![key.clone()],
        auth_rejection_time: Duration::from_millis(5),
        ..Default::default()
    });
    let auth = Arc::new(AtomicUsize::new(0));
    let resizes = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        auth_attempts: auth.clone(),
        public,
        resizes: resizes.clone(),
    };
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let config = config.clone();
            let fixture = fixture.clone();
            tokio::spawn(async move {
                if let Ok(session) = server::run_stream(config, stream, fixture).await {
                    let _ = session.await;
                }
            });
        }
    });
    ServerFixture {
        port,
        key,
        auth,
        resizes,
        task,
    }
}
pub(super) fn state() -> (tempfile::TempDir, Arc<AppState>) {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
    vault.create("disposable master fixture").unwrap();
    let state = Arc::new(AppState {
        vault: Mutex::new(vault),
        access: std::sync::atomic::AtomicU64::new(1),
        confirmations: Mutex::default(),
        streams: Mutex::default(),
        sessions: Mutex::default(),
        imports: Mutex::default(),
        profile_deletes: Mutex::default(),
        docker: Mutex::default(),
        files: Mutex::default(),
        transfers: Mutex::default(),
    });
    (dir, state)
}
pub(super) fn profile(port: u16, mode: AuthMode) -> ProfileInput {
    ProfileInput {
        id: Some(Uuid::new_v4()),
        name: "Fixture".into(),
        host: "127.0.0.1".into(),
        port,
        username: "fixture".into(),
        auth_mode: mode,
        initial_directory: None,
    }
}
fn trusted(server: &ServerFixture) -> KnownHost {
    KnownHost {
        host: "127.0.0.1".into(),
        port: server.port,
        key: server.key.public_key().to_openssh().unwrap(),
        fingerprint: server
            .key
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string(),
    }
}
pub(super) async fn wait_for(state: &AppState, id: Uuid, wanted: &str) -> SessionInfo {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let info = state.sessions.lock().unwrap().entries[&id].info.clone();
            if info.state == wanted {
                return info;
            }
            if info.state == "error" && wanted != "error" {
                panic!("session failed: {:?}", info.error_code);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("session state timeout")
}
async fn open_terminal(
    state: Arc<AppState>,
    id: Uuid,
) -> (
    Uuid,
    tokio::sync::mpsc::UnboundedReceiver<terminal::TerminalEvent>,
) {
    let (events, rx) = tokio::sync::mpsc::unbounded_channel();
    let channel = tauri::ipc::Channel::new(move |event| {
        let tauri::ipc::InvokeResponseBody::Json(json) = event else {
            panic!("expected JSON terminal event")
        };
        let event: terminal::TerminalEvent = serde_json::from_str(&json).unwrap();
        let _ = events.send(event);
        Ok(())
    });
    let (reply, result) = oneshot::channel();
    let sender = state.sessions.lock().unwrap().entries[&id].tx.clone();
    sender
        .send(SessionCommand::OpenTerminal {
            request: terminal::OpenTerminal {
                command: None,
                cols: 80,
                rows: 24,
                channel,
            },
            reply,
        })
        .await
        .unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(10), result)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    (terminal, rx)
}
async fn command(state: &AppState, id: Uuid, command: terminal::TerminalCommand) {
    let tx = state.sessions.lock().unwrap().terminals[&id].tx.clone();
    tx.send(command).await.unwrap();
}
async fn event(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<terminal::TerminalEvent>,
) -> terminal::TerminalEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn host_review_precedes_auth_and_changed_host_needs_explicit_revision() {
    let key = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let server = server(key.public_key().clone()).await;
    let (_dir, state) = state();
    let info = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        None,
    )
    .unwrap();
    let pending = wait_for(&state, info.id, "host_pending").await;
    assert_eq!(server.auth.load(Ordering::SeqCst), 0);
    let challenge = pending.challenge.unwrap();
    assert!(!challenge.changed);
    let tx = state.sessions.lock().unwrap().entries[&info.id].tx.clone();
    tx.send(SessionCommand::Confirm {
        challenge: challenge.id,
        accept: true,
        revision: String::new(),
    })
    .await
    .unwrap();
    wait_for(&state, info.id, "connected").await;
    assert_eq!(server.auth.load(Ordering::SeqCst), 1);
    let other = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let old = KnownHost {
        host: "127.0.0.1".into(),
        port: server.port,
        key: other.public_key().to_openssh().unwrap(),
        fingerprint: other
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string(),
    };
    let changed = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        Some(old.clone()),
    )
    .unwrap();
    let pending = wait_for(&state, changed.id, "host_pending").await;
    assert!(pending.challenge.as_ref().unwrap().changed);
    assert_eq!(server.auth.load(Ordering::SeqCst), 1);
    state.sessions.lock().unwrap().entries[&changed.id]
        .tx
        .try_send(SessionCommand::Confirm {
            challenge: pending.challenge.unwrap().id,
            accept: true,
            revision: "wrong endpoint".into(),
        })
        .unwrap();
    let error = wait_for(&state, changed.id, "error").await;
    assert_eq!(error.error_code, Some("HOST_KEY_CHANGED"));
    assert_eq!(server.auth.load(Ordering::SeqCst), 1);
    state
        .vault
        .lock()
        .unwrap()
        .trust_host(1, old.clone())
        .unwrap();
    let reviewed = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        Some(old),
    )
    .unwrap();
    let pending = wait_for(&state, reviewed.id, "host_pending").await;
    state.sessions.lock().unwrap().entries[&reviewed.id]
        .tx
        .try_send(SessionCommand::Confirm {
            challenge: pending.challenge.unwrap().id,
            accept: true,
            revision: format!("127.0.0.1:{}", server.port),
        })
        .unwrap();
    wait_for(&state, reviewed.id, "connected").await;
    assert_eq!(server.auth.load(Ordering::SeqCst), 2);
    state.sessions.lock().unwrap().clear();
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn terminals_isolate_bytes_apply_backpressure_resize_interrupt_and_close() {
    let key = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let server = server(key.public_key().clone()).await;
    let (_dir, state) = state();
    let a = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        Some(trusted(&server)),
    )
    .unwrap();
    let b = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        Some(trusted(&server)),
    )
    .unwrap();
    wait_for(&state, a.id, "connected").await;
    wait_for(&state, b.id, "connected").await;
    let (ta, mut ea) = open_terminal(state.clone(), a.id).await;
    let (tb, mut eb) = open_terminal(state.clone(), b.id).await;
    let first = event(&mut ea).await;
    assert_eq!(first.bytes, vec![0xc3]);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), ea.recv())
            .await
            .is_err()
    );
    command(&state, ta, terminal::TerminalCommand::Ack(first.sequence)).await;
    let second = event(&mut ea).await;
    assert_eq!(second.bytes, vec![0xa9]);
    command(&state, ta, terminal::TerminalCommand::Ack(second.sequence)).await;
    for _ in 0..2 {
        let chunk = event(&mut eb).await;
        assert_eq!(chunk.session_id, b.id);
        command(&state, tb, terminal::TerminalCommand::Ack(chunk.sequence)).await;
    }
    command(&state, ta, terminal::TerminalCommand::Resize(100, 35)).await;
    command(&state, ta, terminal::TerminalCommand::Input(vec![3])).await;
    let interrupted = event(&mut ea).await;
    assert_eq!(interrupted.bytes, vec![3]);
    assert_eq!(interrupted.session_id, a.id);
    command(
        &state,
        ta,
        terminal::TerminalCommand::Ack(interrupted.sequence),
    )
    .await;
    command(
        &state,
        ta,
        terminal::TerminalCommand::Input(b"flood".to_vec()),
    )
    .await;
    let mut total = 0;
    while total < 1024 * 1024 {
        let chunk = event(&mut ea).await;
        assert!(chunk.bytes.len() <= 65536);
        total += chunk.bytes.len();
        command(&state, ta, terminal::TerminalCommand::Ack(chunk.sequence)).await;
    }
    assert!(server.resizes.lock().unwrap().contains(&(100, 35)));
    command(
        &state,
        ta,
        terminal::TerminalCommand::Input(b"exit".to_vec()),
    )
    .await;
    let closed = event(&mut ea).await;
    assert_eq!(closed.kind, "closed");
    assert_eq!(closed.exit_status, Some(0));
    state.sessions.lock().unwrap().clear();
    let closed = event(&mut eb).await;
    assert_eq!(closed.kind, "closed");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn encrypted_key_password_failure_and_snapshot_are_real() {
    let private = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let server = server(private.public_key().clone()).await;
    let (_dir, state) = state();
    let encrypted = private
        .encrypt(&mut rand10::rng(), "synthetic passphrase")
        .unwrap()
        .to_openssh(LineEnding::LF)
        .unwrap();
    let p = profile(server.port, AuthMode::Key);
    let secret = Secret::Key {
        private_key: encrypted.to_string(),
        passphrase: "synthetic passphrase".into(),
    };
    let info = start(
        state.clone(),
        1,
        p.clone(),
        Some(secret),
        Some(trusted(&server)),
    )
    .unwrap();
    wait_for(&state, info.id, "connected").await;
    let wrong = start(
        state.clone(),
        1,
        p,
        Some(Secret::Key {
            private_key: encrypted.to_string(),
            passphrase: "wrong".into(),
        }),
        Some(trusted(&server)),
    )
    .unwrap();
    assert_eq!(
        wait_for(&state, wrong.id, "error").await.error_code,
        Some("INVALID_KEY")
    );
    let wrong = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "wrong".into(),
        }),
        Some(trusted(&server)),
    )
    .unwrap();
    assert_eq!(
        wait_for(&state, wrong.id, "error").await.error_code,
        Some("AUTH_FAILED")
    );
    let mut p = profile(server.port, AuthMode::Agent);
    p.id = None;
    let saved = state
        .vault
        .lock()
        .unwrap()
        .put_profile(1, p, SecretPolicy::Forget, None)
        .unwrap();
    let snapshot = state
        .vault
        .lock()
        .unwrap()
        .profile(1, saved.fields.id.unwrap())
        .unwrap()
        .fields;
    let active = start(state.clone(), 1, snapshot.clone(), None, None).unwrap();
    let mut edited = snapshot;
    edited.host = "example.invalid".into();
    state
        .vault
        .lock()
        .unwrap()
        .put_profile(1, edited, SecretPolicy::Forget, None)
        .unwrap();
    assert_eq!(
        state.sessions.lock().unwrap().entries[&active.id]
            .info
            .profile
            .host,
        "127.0.0.1"
    );
    state.sessions.lock().unwrap().clear();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires isolated OpenSSH/Agent fixture; run tests/fixtures/ssh/run-native.sh"]
async fn native_openssh() {
    let port = std::env::var("UNNY_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let user = std::env::var("UNNY_TEST_SSH_USER").unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("UNNY_TEST_SSH_DIR").unwrap());
    let (_temp, state) = state();
    let mut p = profile(port, AuthMode::Key);
    p.username = user.clone();
    let secret = Secret::Key {
        private_key: std::fs::read_to_string(dir.join("id_ed25519")).unwrap(),
        passphrase: String::new(),
    };
    let info = start(state.clone(), 1, p.clone(), Some(secret), None).unwrap();
    let pending = wait_for(&state, info.id, "host_pending").await;
    let challenge = pending.challenge.unwrap();
    let tx = state.sessions.lock().unwrap().entries[&info.id].tx.clone();
    tx.send(SessionCommand::Confirm {
        challenge: challenge.id,
        accept: true,
        revision: String::new(),
    })
    .await
    .unwrap();
    assert!(wait_for(&state, info.id, "connected").await.sftp);
    let known = state
        .vault
        .lock()
        .unwrap()
        .known_host(1, "127.0.0.1", port)
        .unwrap();
    let rsa = start(
        state.clone(),
        1,
        p.clone(),
        Some(Secret::Key {
            private_key: std::fs::read_to_string(dir.join("id_rsa")).unwrap(),
            passphrase: "disposable fixture passphrase".into(),
        }),
        known.clone(),
    )
    .unwrap();
    wait_for(&state, rsa.id, "connected").await;
    p.auth_mode = AuthMode::Agent;
    let agent = start(state.clone(), 1, p, None, known).unwrap();
    wait_for(&state, agent.id, "connected").await;
    let (terminal, mut rx) = open_terminal(state.clone(), info.id).await;
    command(&state, terminal, terminal::TerminalCommand::Resize(91, 33)).await;
    command(
        &state,
        terminal,
        terminal::TerminalCommand::Input(
            b"stty size; printf '\\125NNY\\137PTY\\137OK\\n'\r".to_vec(),
        ),
    )
    .await;
    let mut received = Vec::new();
    loop {
        let event = event(&mut rx).await;
        assert_eq!(event.kind, "data");
        received.extend_from_slice(&event.bytes);
        command(
            &state,
            terminal,
            terminal::TerminalCommand::Ack(event.sequence),
        )
        .await;
        if String::from_utf8_lossy(&received).contains("UNNY_PTY_OK") {
            break;
        }
    }
    assert!(String::from_utf8_lossy(&received).contains("33 91"));
    command(
        &state,
        terminal,
        terminal::TerminalCommand::Input(b"sleep 10\r".to_vec()),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    command(&state, terminal, terminal::TerminalCommand::Input(vec![3])).await;
    command(
        &state,
        terminal,
        terminal::TerminalCommand::Input(b"printf '\\125NNY\\137INT\\137OK\\n'\r".to_vec()),
    )
    .await;
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = event(&mut rx).await;
            received.extend_from_slice(&event.bytes);
            command(
                &state,
                terminal,
                terminal::TerminalCommand::Ack(event.sequence),
            )
            .await;
            if String::from_utf8_lossy(&received).contains("UNNY_INT_OK") {
                break;
            }
        }
    })
    .await
    .expect("Ctrl+C did not interrupt the remote sleep");
    state.sessions.lock().unwrap().clear();
}
#[test]
#[ignore = "seeds only the isolated native smoke directory"]
fn native_seed() {
    let path = std::path::PathBuf::from(
        std::env::var_os("UNNY_SMOKE_DATA_DIR").expect("isolated smoke directory required"),
    );
    let dir = std::path::PathBuf::from(std::env::var_os("UNNY_TEST_SSH_DIR").unwrap());
    let mut vault = Vault::new(Storage::open(path).unwrap());
    vault.create("unny disposable smoke fixture").unwrap();
    let mut p = profile(
        std::env::var("UNNY_TEST_SSH_PORT")
            .unwrap()
            .parse()
            .unwrap(),
        AuthMode::Key,
    );
    p.id = None;
    p.name = "OpenSSH descartável".into();
    p.initial_directory = Some(dir.join("remote").to_string_lossy().into_owned());
    p.username = std::env::var("UNNY_TEST_SSH_USER").unwrap();
    let host_key = russh::keys::PublicKey::from_openssh(
        &std::fs::read_to_string(dir.join("host.pub")).unwrap(),
    )
    .unwrap();
    vault
        .trust_host(
            1,
            KnownHost {
                host: "127.0.0.1".into(),
                port: p.port,
                key: host_key.to_openssh().unwrap(),
                fingerprint: host_key
                    .fingerprint(russh::keys::HashAlg::Sha256)
                    .to_string(),
            },
        )
        .unwrap();
    vault
        .put_profile(
            1,
            p,
            SecretPolicy::Save,
            Some(Secret::Key {
                private_key: std::fs::read_to_string(dir.join("id_ed25519")).unwrap(),
                passphrase: String::new(),
            }),
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn identity_expiration_rejection_and_network_timeout_do_not_authenticate() {
    let private = PrivateKey::random(&mut rand10::rng(), Algorithm::Ed25519).unwrap();
    let server = server(private.public_key().clone()).await;
    let (_dir, state) = state();
    let expired = start_with_deadlines(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        None,
        Deadlines {
            connect: Duration::from_secs(2),
            confirmation: Duration::from_millis(150),
        },
    )
    .unwrap();
    assert_eq!(
        wait_for(&state, expired.id, "error").await.error_code,
        Some("CONFIRMATION_EXPIRED")
    );
    assert_eq!(server.auth.load(Ordering::SeqCst), 0);
    let rejected = start(
        state.clone(),
        1,
        profile(server.port, AuthMode::Password),
        Some(Secret::Password {
            password: "synthetic-only".into(),
        }),
        None,
    )
    .unwrap();
    let pending = wait_for(&state, rejected.id, "host_pending").await;
    state.sessions.lock().unwrap().entries[&rejected.id]
        .tx
        .try_send(SessionCommand::Confirm {
            challenge: pending.challenge.unwrap().id,
            accept: false,
            revision: String::new(),
        })
        .unwrap();
    assert_eq!(
        wait_for(&state, rejected.id, "error").await.error_code,
        Some("HOST_REJECTED")
    );
    assert_eq!(server.auth.load(Ordering::SeqCst), 0);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let stalled = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let timeout = start_with_deadlines(
        state.clone(),
        1,
        profile(port, AuthMode::Agent),
        None,
        None,
        Deadlines {
            connect: Duration::from_millis(100),
            confirmation: Duration::from_secs(1),
        },
    )
    .unwrap();
    assert_eq!(
        wait_for(&state, timeout.id, "error").await.error_code,
        Some("TIMEOUT")
    );
    stalled.abort();
    state.sessions.lock().unwrap().clear();
    state.access.store(0, Ordering::SeqCst);
    assert_eq!(
        start(
            state.clone(),
            1,
            profile(server.port, AuthMode::Agent),
            None,
            None
        )
        .unwrap_err()
        .code,
        "VAULT_LOCKED"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg(unix)]
#[ignore = "requires isolated native SFTP fixture: run-native.sh --files"]
async fn native_files() {
    use crate::services::{files, transfers};
    use std::os::unix::fs::PermissionsExt;
    let port = std::env::var("UNNY_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("UNNY_TEST_SSH_DIR").unwrap());
    let remote = root.join("remote");
    let (_tmp, state) = state();
    let mut p = profile(port, AuthMode::Key);
    p.username = std::env::var("UNNY_TEST_SSH_USER").unwrap();
    p.initial_directory = Some(remote.to_string_lossy().to_string());
    let host =
        PublicKey::from_openssh(&std::fs::read_to_string(root.join("host.pub")).unwrap()).unwrap();
    let known = KnownHost {
        host: "127.0.0.1".into(),
        port,
        key: host.to_openssh().unwrap(),
        fingerprint: host.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
    };
    let secret = Secret::Key {
        private_key: std::fs::read_to_string(root.join("id_ed25519")).unwrap(),
        passphrase: String::new(),
    };
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
    let listing = files::list(&ctx, None, None).await.unwrap();
    let directory = listing.directory.id;
    let hello = listing
        .entries
        .iter()
        .find(|e| e.name == "hello.txt")
        .unwrap();
    if let Some(nonutf) = listing.entries.iter().find(|e| e.escaped) {
        assert_eq!(nonutf.name, "bad_\\xFF.txt");
        assert!(matches!(
            files::open_document(&ctx, nonutf.id).await.unwrap(),
            files::OpenReply::Document { .. }
        ));
    } else {
        assert!(root.join("nonutf8-unavailable").exists());
    }
    let created = files::mutate(
        &ctx,
        files::Mutation::Create {
            parent: directory,
            name: "created file.txt".into(),
            directory: false,
        },
    )
    .await
    .unwrap();
    let files::MutationReply::Created { entry } = created else {
        panic!("created")
    };
    let files::MutationReply::Confirmation { token, .. } = files::mutate(
        &ctx,
        files::Mutation::Prepare {
            entry: entry.id,
            name: Some("renamed ' file.txt".into()),
        },
    )
    .await
    .unwrap() else {
        panic!("confirmation")
    };
    files::mutate(&ctx, files::Mutation::Apply { token })
        .await
        .unwrap();
    assert!(remote.join("renamed ' file.txt").exists());
    assert!(
        files::mutate(&ctx, files::Mutation::Apply { token })
            .await
            .is_err()
    );
    let page = files::list(&ctx, None, None).await.unwrap();
    let renamed = page
        .entries
        .iter()
        .find(|e| e.name == "renamed ' file.txt")
        .unwrap();
    let files::MutationReply::Confirmation { token, .. } = files::mutate(
        &ctx,
        files::Mutation::Prepare {
            entry: renamed.id,
            name: None,
        },
    )
    .await
    .unwrap() else {
        panic!("confirmation")
    };
    files::mutate(&ctx, files::Mutation::Apply { token })
        .await
        .unwrap();
    assert!(!remote.join("renamed ' file.txt").exists());
    let no_access = page.entries.iter().find(|e| e.name == "no-access").unwrap();
    if std::env::var("UNNY_TEST_SSH_USER").unwrap() != "root" {
        assert_eq!(
            files::list(&ctx, Some(no_access.id), None)
                .await
                .err()
                .unwrap()
                .code,
            "PERMISSION_DENIED"
        );
    }
    std::fs::set_permissions(
        remote.join("no-access"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let nested = page.entries.iter().find(|e| e.name == "nested").unwrap();
    let files::MutationReply::Confirmation { token, .. } = files::mutate(
        &ctx,
        files::Mutation::Prepare {
            entry: nested.id,
            name: None,
        },
    )
    .await
    .unwrap() else {
        panic!("confirm")
    };
    assert!(
        files::mutate(&ctx, files::Mutation::Apply { token })
            .await
            .is_err()
    );
    assert!(remote.join("nested/inside.txt").exists());
    let files::OpenReply::Document { document } =
        files::open_document(&ctx, hello.id).await.unwrap()
    else {
        panic!("document")
    };
    assert!(document.bom);
    assert_eq!(document.line_endings, "CRLF");
    assert_eq!(document.text, "hello\nworld\n");
    assert!(matches!(
        files::save_document(
            &ctx,
            document.id,
            document.revision.clone(),
            "edited\n".into(),
            None
        )
        .await
        .unwrap(),
        files::SaveReply::Saved { .. }
    ));
    assert_eq!(
        std::fs::read(remote.join("hello.txt")).unwrap(),
        b"\xef\xbb\xbfedited\r\n"
    );
    assert_eq!(
        std::fs::metadata(remote.join("hello.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    let files::OpenReply::Document { document } =
        files::open_document(&ctx, hello.id).await.unwrap()
    else {
        panic!("document")
    };
    std::fs::write(remote.join("hello.txt"), b"external\n").unwrap();
    let files::SaveReply::Conflict { token, .. } = files::save_document(
        &ctx,
        document.id,
        document.revision.clone(),
        "mine\n".into(),
        None,
    )
    .await
    .unwrap() else {
        panic!("conflict")
    };
    assert_eq!(
        std::fs::read(remote.join("hello.txt")).unwrap(),
        b"external\n"
    );
    assert!(matches!(
        files::save_document(
            &ctx,
            document.id,
            document.revision.clone(),
            "mine\n".into(),
            Some(token)
        )
        .await
        .unwrap(),
        files::SaveReply::Saved { .. }
    ));
    let files::OpenReply::Document { document } =
        files::open_document(&ctx, hello.id).await.unwrap()
    else {
        panic!("document")
    };
    std::fs::remove_file(remote.join("hello.txt")).unwrap();
    let files::SaveReply::Conflict { token, missing, .. } = files::save_document(
        &ctx,
        document.id,
        document.revision.clone(),
        "restored\n".into(),
        None,
    )
    .await
    .unwrap() else {
        panic!("missing conflict")
    };
    assert!(missing);
    assert!(!remote.join("hello.txt").exists());
    assert!(matches!(
        files::save_document(
            &ctx,
            document.id,
            document.revision.clone(),
            "restored\n".into(),
            Some(token)
        )
        .await
        .unwrap(),
        files::SaveReply::Saved { .. }
    ));
    if std::env::var("UNNY_TEST_SSH_USER").unwrap() != "root" {
        let files::OpenReply::Document { document } =
            files::open_document(&ctx, hello.id).await.unwrap()
        else {
            panic!("document")
        };
        let before = std::fs::read(remote.join("hello.txt")).unwrap();
        std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = files::save_document(
            &ctx,
            document.id,
            document.revision,
            "denied\n".into(),
            None,
        )
        .await;
        std::fs::set_permissions(&remote, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(result,Ok(files::SaveReply::Failed{error,..}) if error.code=="PERMISSION_DENIED")
        );
        assert_eq!(std::fs::read(remote.join("hello.txt")).unwrap(), before);
    }
    let binary = listing
        .entries
        .iter()
        .find(|e| e.name == "binary.bin")
        .unwrap();
    assert!(matches!(
        files::open_document(&ctx, binary.id).await.unwrap(),
        files::OpenReply::Restricted { .. }
    ));
    let large = listing
        .entries
        .iter()
        .find(|e| e.name == "large.bin")
        .unwrap();
    assert!(matches!(
        files::open_document(&ctx, large.id).await.unwrap(),
        files::OpenReply::Restricted { .. }
    ));
    let b = start(state.clone(), 1, p, Some(secret), Some(known)).unwrap();
    wait_for(&state, b.id, "connected").await;
    let other = files::context(state.clone(), b.id, 1).await.unwrap();
    assert!(other.path(hello.id).is_err());
    let temp = tempfile::tempdir().unwrap();
    let destination =
        transfers::Transfers::prepare_destination(temp.path().join("large.bin")).unwrap();
    let op = transfers::start_download(
        state.clone(),
        ctx.clone(),
        large.id,
        destination,
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .unwrap();
    let done = wait_transfer(&state, session.id, op.id).await;
    assert_eq!(done.state, "completed", "{:?}", done);
    assert_eq!(
        std::fs::metadata(temp.path().join("large.bin"))
            .unwrap()
            .len(),
        8 * 1024 * 1024
    );
    let destination =
        transfers::Transfers::prepare_destination(temp.path().join("tree.zip")).unwrap();
    let op = transfers::start_download(
        state.clone(),
        ctx.clone(),
        directory,
        destination,
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .unwrap();
    let done = wait_transfer(&state, session.id, op.id).await;
    assert_eq!(done.state, "completed", "{:?}", done);
    assert_eq!(done.ignored_count, 1);
    let mut zip =
        zip::ZipArchive::new(std::fs::File::open(temp.path().join("tree.zip")).unwrap()).unwrap();
    for i in 0..zip.len() {
        let file = zip.by_index(i).unwrap();
        assert!(!file.name().unwrap().contains("link-outside"));
        assert!(!file.name().unwrap().contains("../"));
    }
    assert!(zip.by_name("remote/nested/inside.txt").is_ok());
    let local = temp.path().join("hello.txt");
    std::fs::write(&local, b"upload body").unwrap();
    let source = transfers::Transfers::open_sources(vec![local])
        .unwrap()
        .pop()
        .unwrap();
    let op = transfers::start_upload(
        state.clone(),
        ctx.clone(),
        source,
        directory,
        tauri::ipc::Channel::new(|_| Ok(())),
    )
    .unwrap();
    let waiting = wait_collision(&state, session.id, op.id).await;
    state
        .transfers
        .lock()
        .unwrap()
        .resolve(
            op.id,
            session.id,
            1,
            waiting.collision.unwrap().token,
            transfers::Decision::Rename {
                name: "uploaded.txt".into(),
            },
        )
        .unwrap();
    let done = wait_transfer(&state, session.id, op.id).await;
    assert_eq!(done.state, "completed", "{:?}", done);
    assert_eq!(
        std::fs::read(remote.join("uploaded.txt")).unwrap(),
        b"upload body"
    );
    // Hold both session permits through two collision decisions; a third stays queued.
    let mut jobs = Vec::new();
    for _ in 0..3 {
        let source = transfers::Transfers::open_sources(vec![temp.path().join("hello.txt")])
            .unwrap()
            .pop()
            .unwrap();
        jobs.push(
            transfers::start_upload(
                state.clone(),
                ctx.clone(),
                source,
                directory,
                tauri::ipc::Channel::new(|_| Ok(())),
            )
            .unwrap(),
        );
    }
    let first = wait_collision(&state, session.id, jobs[0].id).await;
    let second = wait_collision(&state, session.id, jobs[1].id).await;
    assert_eq!(
        state
            .transfers
            .lock()
            .unwrap()
            .list(session.id, 1)
            .into_iter()
            .find(|s| s.id == jobs[2].id)
            .unwrap()
            .state,
        "queued"
    );
    state
        .transfers
        .lock()
        .unwrap()
        .resolve(
            jobs[0].id,
            session.id,
            1,
            first.collision.unwrap().token,
            transfers::Decision::Skip,
        )
        .unwrap();
    assert_eq!(
        wait_transfer(&state, session.id, jobs[0].id).await.state,
        "skipped"
    );
    wait_collision(&state, session.id, jobs[2].id).await;
    state
        .transfers
        .lock()
        .unwrap()
        .cancel(jobs[2].id, session.id, 1)
        .unwrap();
    assert_eq!(
        wait_transfer(&state, session.id, jobs[2].id).await.state,
        "cancelled"
    );
    state
        .transfers
        .lock()
        .unwrap()
        .resolve(
            jobs[1].id,
            session.id,
            1,
            second.collision.unwrap().token,
            transfers::Decision::Overwrite {
                allow_non_atomic: false,
            },
        )
        .unwrap();
    assert_eq!(
        wait_transfer(&state, session.id, jobs[1].id).await.state,
        "completed"
    );
    assert_eq!(
        std::fs::read(remote.join("hello.txt")).unwrap(),
        b"upload body"
    );
    state.sessions.lock().unwrap().clear();
}
async fn wait_transfer(
    state: &AppState,
    session: Uuid,
    id: Uuid,
) -> crate::services::transfers::Snapshot {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let info = state
                .transfers
                .lock()
                .unwrap()
                .list(session, 1)
                .into_iter()
                .find(|s| s.id == id)
                .unwrap();
            if !info.active() {
                return info;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn wait_collision(
    state: &AppState,
    session: Uuid,
    id: Uuid,
) -> crate::services::transfers::Snapshot {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let info = state
                .transfers
                .lock()
                .unwrap()
                .list(session, 1)
                .into_iter()
                .find(|s| s.id == id)
                .unwrap();
            if info.collision.is_some() {
                return info;
            }
            assert!(info.active(), "{:?}", info);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
