use crate::{
    adapters::ssh::{self, CONNECT_TIMEOUT, Verifier},
    commands::AppState,
    ipc::{AppError, Result},
    services::profiles::{KnownHost, ProfileInput, Secret},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub mod terminal;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostChallenge {
    pub id: Uuid,
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub previous_fingerprint: Option<String>,
    pub changed: bool,
    pub algorithm: String,
    pub expires_in_seconds: u32,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: Uuid,
    pub generation: u64,
    pub profile: ProfileInput,
    pub state: &'static str,
    pub error_code: Option<&'static str>,
    pub error_message: Option<&'static str>,
    pub challenge: Option<HostChallenge>,
    pub sftp: bool,
}
pub enum SessionCommand {
    Confirm {
        challenge: Uuid,
        accept: bool,
        revision: String,
    },
    OpenExec {
        command: String,
        reply: oneshot::Sender<Result<russh::Channel<russh::client::Msg>>>,
    },
    OpenSftp {
        reply: oneshot::Sender<Result<russh::Channel<russh::client::Msg>>>,
    },
    OpenTerminal {
        request: terminal::OpenTerminal,
        reply: oneshot::Sender<Result<Uuid>>,
    },
}
pub struct SessionEntry {
    pub info: SessionInfo,
    pub tx: mpsc::Sender<SessionCommand>,
    pub cancel: CancellationToken,
}
#[derive(Default)]
pub struct Sessions {
    pub entries: HashMap<Uuid, SessionEntry>,
    pub terminals: HashMap<Uuid, terminal::TerminalEntry>,
}
impl Sessions {
    pub fn authorize(&self, id: Uuid, generation: u64) -> Result<&SessionEntry> {
        let entry = self.entries.get(&id).ok_or_else(unknown)?;
        if entry.info.generation != generation {
            return Err(expired());
        }
        Ok(entry)
    }
    pub fn clear(&mut self) {
        for entry in self.entries.values() {
            entry.cancel.cancel();
        }
        for entry in self.terminals.values() {
            entry.cancel.cancel();
        }
        self.entries.clear();
        self.terminals.clear();
    }
    pub fn disconnect(&mut self, id: Uuid, generation: u64) -> Result<()> {
        let entry = self.authorize(id, generation)?;
        entry.cancel.cancel();
        for terminal in self.terminals.values().filter(|t| t.session_id == id) {
            terminal.cancel.cancel();
        }
        let entry = self.entries.get_mut(&id).ok_or_else(unknown)?;
        entry.info.state = "disconnected";
        entry.info.challenge = None;
        Ok(())
    }
}
pub fn unknown() -> AppError {
    AppError::new(
        "UNKNOWN_RESOURCE",
        "Sessão ou terminal desconhecido ou encerrado.",
    )
}
pub fn expired() -> AppError {
    AppError::new(
        "STALE_GENERATION",
        "Esta sessão expirou. Conecte novamente.",
    )
}
#[derive(Clone, Copy)]
struct Deadlines {
    connect: Duration,
    confirmation: Duration,
}
impl Default for Deadlines {
    fn default() -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            confirmation: Duration::from_secs(300),
        }
    }
}
pub fn start(
    state: Arc<AppState>,
    generation: u64,
    profile: ProfileInput,
    secret: Option<Secret>,
    known: Option<KnownHost>,
) -> Result<SessionInfo> {
    start_with_deadlines(
        state,
        generation,
        profile,
        secret,
        known,
        Deadlines::default(),
    )
}
fn start_with_deadlines(
    state: Arc<AppState>,
    generation: u64,
    profile: ProfileInput,
    secret: Option<Secret>,
    known: Option<KnownHost>,
    deadlines: Deadlines,
) -> Result<SessionInfo> {
    state.authorize(generation)?;
    let (tx, rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let info = SessionInfo {
        id: Uuid::new_v4(),
        generation,
        profile: profile.clone(),
        state: "connecting",
        error_code: None,
        error_message: None,
        challenge: None,
        sftp: false,
    };
    {
        let mut sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        if sessions.entries.len() >= 16 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Feche uma sessão antes de abrir outra (limite: 16).",
            ));
        }
        sessions.entries.insert(
            info.id,
            SessionEntry {
                info: info.clone(),
                tx,
                cancel: cancel.clone(),
            },
        );
    }
    let id = info.id;
    tauri::async_runtime::spawn(async move {
        let outcome = run(
            state.clone(),
            id,
            generation,
            profile,
            secret,
            known,
            rx,
            cancel.clone(),
            deadlines,
        )
        .await;
        let manually_cancelled = cancel.is_cancelled();
        cancel.cancel();
        if let Ok(mut sessions) = state.sessions.lock() {
            for terminal in sessions.terminals.values().filter(|t| t.session_id == id) {
                terminal.cancel.cancel();
            }
            if let Some(entry) = sessions.entries.get_mut(&id) {
                entry.info.challenge = None;
                if manually_cancelled {
                    entry.info.state = "disconnected";
                } else if let Err(error) = outcome {
                    entry.info.state = "error";
                    entry.info.error_code = Some(error.code);
                    entry.info.error_message = Some(error.message);
                } else {
                    entry.info.state = "disconnected";
                }
            }
        }
    });
    Ok(info)
}
fn update(state: &AppState, id: Uuid, update: impl FnOnce(&mut SessionInfo)) {
    if let Ok(mut sessions) = state.sessions.lock() {
        if let Some(entry) = sessions.entries.get_mut(&id) {
            if !entry.cancel.is_cancelled() {
                update(&mut entry.info);
            }
        }
    }
}
async fn run(
    state: Arc<AppState>,
    id: Uuid,
    generation: u64,
    profile: ProfileInput,
    secret: Option<Secret>,
    mut known: Option<KnownHost>,
    mut rx: mpsc::Receiver<SessionCommand>,
    cancel: CancellationToken,
    deadlines: Deadlines,
) -> Result<()> {
    let mut handle = loop {
        let observed = Arc::new(Mutex::new(None));
        let unsupported = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = Verifier {
            expected: known.as_ref().map(|k| k.key.clone()),
            observed: observed.clone(),
            unsupported: unsupported.clone(),
        };
        let config = russh::client::Config {
            window_size: 256 * 1024,
            maximum_packet_size: 32768,
            channel_buffer_size: 8,
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            ..Default::default()
        };
        update(&state, id, |info| {
            info.state = "connecting";
            info.challenge = None;
        });
        let connection = tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(deadlines.connect,ssh::connect(Arc::new(config),profile.host.as_str(),profile.port,handler,cancel.clone()))=>result};
        match connection {
            Ok(Ok(handle)) => break handle,
            Err(_) => {
                return Err(AppError::new(
                    "TIMEOUT",
                    "O servidor não respondeu em 15 segundos. Confira a rede/VPN.",
                ));
            }
            Ok(Err(_)) => {
                if unsupported.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(AppError::new(
                        "UNSUPPORTED",
                        "Certificados de host SSH não são suportados nesta versão.",
                    ));
                }
                let captured = observed.lock().map_err(|_| AppError::io())?.clone();
                let Some((key, fingerprint)) = captured else {
                    return Err(ssh::network());
                };
                if known
                    .as_ref()
                    .is_some_and(|expected| ssh::same_public_key(&expected.key, &key))
                {
                    return Err(ssh::network());
                }
                let challenge = HostChallenge {
                    id: Uuid::new_v4(),
                    host: profile.host.clone(),
                    port: profile.port,
                    fingerprint: fingerprint.clone(),
                    previous_fingerprint: known.as_ref().map(|k| k.fingerprint.clone()),
                    changed: known.is_some(),
                    algorithm: key.split_whitespace().next().unwrap_or("SSH").to_owned(),
                    expires_in_seconds: 300,
                };
                update(&state, id, |info| {
                    info.state = "host_pending";
                    info.challenge = Some(challenge.clone());
                });
                let decision = tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(deadlines.confirmation,rx.recv())=>result};
                let Ok(Some(SessionCommand::Confirm {
                    challenge: token,
                    accept,
                    revision,
                })) = decision
                else {
                    return Err(AppError::new(
                        "CONFIRMATION_EXPIRED",
                        "A confirmação de identidade expirou. Conecte novamente.",
                    ));
                };
                if token != challenge.id {
                    return Err(AppError::invalid());
                }
                if !accept {
                    return Err(AppError::new(
                        "HOST_REJECTED",
                        "Identidade rejeitada. Nenhuma credencial foi enviada.",
                    ));
                }
                if challenge.changed && revision != format!("{}:{}", profile.host, profile.port) {
                    return Err(AppError::new(
                        "HOST_KEY_CHANGED",
                        "Confirme o endereço do servidor para substituir sua identidade conhecida.",
                    ));
                }
                let trusted = KnownHost {
                    host: profile.host.clone(),
                    port: profile.port,
                    key,
                    fingerprint,
                };
                let saved = trusted.clone();
                let expected_key = known.as_ref().map(|host| host.key.clone());
                let state_copy = state.clone();
                tokio::task::spawn_blocking(move || {
                    state_copy
                        .vault
                        .lock()
                        .map_err(|_| AppError::io())?
                        .trust_host_if_unchanged(generation, expected_key, saved)
                })
                .await
                .map_err(|_| AppError::io())??;
                if cancel.is_cancelled() {
                    return Ok(());
                }
                known = Some(trusted);
            }
        }
    };
    tokio::select! {biased;_=cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(CONNECT_TIMEOUT,ssh::authenticate(&mut handle,&profile,secret))=>result.map_err(|_|AppError::new("TIMEOUT","Autenticação excedeu o tempo limite."))??};
    // Probe only the SFTP subsystem; failure preserves the SSH/PTY connection.
    let sftp = tokio::select! {_=cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(5),async {
        let channel=handle.channel_open_session().await.map_err(|_|())?;
        channel.request_subsystem(true,"sftp").await.map_err(|_|())?;
        let sftp=russh_sftp::client::SftpSession::new(channel.into_stream()).await.map_err(|_|())?;
        let _=sftp.close().await;Ok::<_,()>(())
    })=>matches!(result,Ok(Ok(())))};
    update(&state, id, |info| {
        info.state = "connected";
        info.sftp = sftp;
    });
    let mut poll = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {biased;
            _=cancel.cancelled()=>{let _=tokio::time::timeout(Duration::from_secs(1),handle.disconnect(russh::Disconnect::ByApplication,"","")).await;return Ok(());},
            _=poll.tick()=>{if handle.is_closed(){return Err(AppError::new("CONNECTION_LOST","A conexão SSH foi encerrada. Reconecte manualmente."));}},
            command=rx.recv()=>match command {
                Some(SessionCommand::OpenExec{command,reply})=>{
                    let result=tokio::select!{_=cancel.cancelled()=>Err(expired()),result=tokio::time::timeout(CONNECT_TIMEOUT,async{
                        let mut channel=handle.channel_open_session().await.map_err(|_|unknown())?;
                        channel.exec(true,command).await.map_err(|_|unknown())?;
                        loop{match channel.wait().await{Some(russh::ChannelMsg::Success)=>return Ok(channel),Some(russh::ChannelMsg::Failure)|Some(russh::ChannelMsg::Close)|None=>return Err(AppError::new("EXEC_UNAVAILABLE","O servidor recusou a execução SSH.")),_=>{}}}
                    })=>result.map_err(|_|AppError::new("TIMEOUT","O servidor não abriu execução SSH.")).and_then(|r|r)};
                    let _=reply.send(result);
                },
                Some(SessionCommand::OpenSftp{reply})=>{
                    let result=tokio::select!{_=cancel.cancelled()=>Err(expired()),result=tokio::time::timeout(CONNECT_TIMEOUT,async {
                        let mut channel=handle.channel_open_session().await.map_err(|_|unknown())?;
                        channel.request_subsystem(true,"sftp").await.map_err(|_|unknown())?;
                        loop{match channel.wait().await{Some(russh::ChannelMsg::Success)=>return Ok(channel),Some(russh::ChannelMsg::Failure)|Some(russh::ChannelMsg::Close)|None=>return Err(AppError::new("SFTP_UNAVAILABLE","SFTP indisponível nesta sessão. O terminal continua disponível.")),_=>{}}}
                    })=>result.map_err(|_|AppError::new("TIMEOUT","O canal SFTP não respondeu.")).and_then(|r|r)};
                    let _=reply.send(result);
                },
                Some(SessionCommand::OpenTerminal{request,reply})=>{
                    let result=tokio::select!{_=cancel.cancelled()=>Err(expired()),result=tokio::time::timeout(CONNECT_TIMEOUT,terminal::open(state.clone(),&handle,id,generation,profile.initial_directory.clone(),cancel.child_token(),request))=>result.map_err(|_|AppError::new("TIMEOUT","A abertura do terminal excedeu o tempo limite.")).and_then(|r|r)};
                    let _=reply.send(result);
                },
                None=>return Ok(()),
                _=>{},
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub session_id: Uuid,
    pub generation: u64,
}

#[cfg(test)]
mod tests;

#[cfg(all(test, unix))]
mod docker_tests;
