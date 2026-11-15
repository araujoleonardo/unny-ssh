use super::{expired, unknown};
use crate::{
    adapters::ssh::SshHandle,
    commands::AppState,
    ipc::{AppError, Result},
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tauri::ipc::Channel;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalEvent {
    pub terminal_id: Uuid,
    pub session_id: Uuid,
    pub generation: u64,
    pub sequence: u64,
    pub kind: String,
    pub bytes: Vec<u8>,
    pub exit_status: Option<u32>,
}
pub struct OpenTerminal {
    pub command: Option<Vec<u8>>,
    pub cols: u32,
    pub rows: u32,
    pub channel: Channel<TerminalEvent>,
}
pub enum TerminalCommand {
    Input(Vec<u8>),
    Resize(u32, u32),
    Ack(u64),
}
pub struct TerminalEntry {
    pub session_id: Uuid,
    pub generation: u64,
    pub tx: mpsc::Sender<TerminalCommand>,
    pub cancel: CancellationToken,
}
pub fn dimensions(cols: u32, rows: u32) -> Result<()> {
    if !(2..=500).contains(&cols) || !(1..=300).contains(&rows) {
        return Err(AppError::invalid());
    }
    Ok(())
}
pub fn quote_path(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}
/// Build an exec request, preserving opaque SFTP bytes instead of typing into a PTY.
pub fn directory_command(path: &[u8]) -> Result<Vec<u8>> {
    if !path.starts_with(b"/") || path.contains(&0) || path.len() > 16384 {
        return Err(AppError::invalid());
    }
    let mut command = b"cd -- '".to_vec();
    for byte in path {
        if *byte == b'\'' {
            command.extend_from_slice(b"'\\''");
        } else {
            command.push(*byte);
        }
    }
    command.extend_from_slice(b"' && exec \"${SHELL:-/bin/sh}\" -i");
    Ok(command)
}
#[cfg(test)]
mod directory_tests {
    use super::*;
    #[test]
    fn quotes_opaque_paths_without_shell_expansion() {
        assert_eq!(
            directory_command(b"/a'$(touch x);\xff").unwrap(),
            b"cd -- '/a'\\''$(touch x);\xff' && exec \"${SHELL:-/bin/sh}\" -i"
        );
        assert!(directory_command(b"/a\0b").is_err());
        assert!(directory_command(b"relative").is_err());
    }
}
async fn success(channel: &mut russh::Channel<russh::client::Msg>) -> Result<()> {
    loop {
        match channel.wait().await {
            Some(russh::ChannelMsg::Success) => return Ok(()),
            Some(russh::ChannelMsg::Failure) | Some(russh::ChannelMsg::Close) | None => {
                return Err(AppError::new(
                    "PTY_UNAVAILABLE",
                    "O servidor recusou o terminal interativo.",
                ));
            }
            _ => {}
        }
    }
}
pub async fn open(
    state: Arc<AppState>,
    handle: &SshHandle,
    session_id: Uuid,
    generation: u64,
    directory: Option<String>,
    cancel: CancellationToken,
    request: OpenTerminal,
) -> Result<Uuid> {
    state.authorize(generation)?;
    dimensions(request.cols, request.rows)?;
    {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        if sessions.terminals.len() >= 32
            || sessions
                .terminals
                .values()
                .filter(|t| t.session_id == session_id)
                .count()
                >= 8
        {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Limite de terminais atingido (8 por sessão, 32 no total).",
            ));
        }
    }
    let mut channel = handle.channel_open_session().await.map_err(|_| unknown())?;
    channel
        .request_pty(
            true,
            "xterm-256color",
            request.cols,
            request.rows,
            0,
            0,
            &[],
        )
        .await
        .map_err(|_| unknown())?;
    success(&mut channel).await?;
    let shell = request.command.is_none();
    if let Some(command) = request.command {
        channel.exec(true, command).await.map_err(|_| unknown())?;
    } else {
        channel.request_shell(true).await.map_err(|_| unknown())?;
    }
    success(&mut channel).await?;
    if let Some(path) = directory.filter(|_| shell) {
        channel
            .data_bytes(format!("cd -- {}\r", quote_path(&path)).into_bytes())
            .await
            .map_err(|_| unknown())?;
    }
    let id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel(32);
    {
        state.authorize(generation)?;
        let mut sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        sessions.authorize(session_id, generation)?;
        if cancel.is_cancelled() {
            return Err(expired());
        }
        sessions.terminals.insert(
            id,
            TerminalEntry {
                session_id,
                generation,
                tx,
                cancel: cancel.clone(),
            },
        );
    }
    tauri::async_runtime::spawn(async move {
        run(
            channel,
            rx,
            cancel,
            request.channel,
            id,
            session_id,
            generation,
        )
        .await;
        if let Ok(mut sessions) = state.sessions.lock() {
            sessions.terminals.remove(&id);
        }
    });
    Ok(id)
}
async fn run(
    mut remote: russh::Channel<russh::client::Msg>,
    mut commands: mpsc::Receiver<TerminalCommand>,
    cancel: CancellationToken,
    channel: Channel<TerminalEvent>,
    id: Uuid,
    session: Uuid,
    generation: u64,
) {
    let mut sequence = 0;
    let mut waiting = false;
    let mut exit = None;
    let mut failed = false;
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {biased;
            _=cancel.cancelled()=>break,
            _=tokio::time::sleep_until(deadline),if waiting=>{failed=true;break;},
            command=commands.recv()=>match command {
                Some(TerminalCommand::Ack(n))=>{if waiting && n==sequence {waiting=false;}else{failed=true;break;}},
                Some(TerminalCommand::Input(bytes))=>{
                    let sent=tokio::select!{_=cancel.cancelled()=>break,result=tokio::time::timeout(Duration::from_secs(5),remote.data_bytes(bytes))=>result};
                    if !matches!(sent,Ok(Ok(()))){failed=true;break;}
                },
                Some(TerminalCommand::Resize(cols,rows))=>{
                    let result=tokio::select!{_=cancel.cancelled()=>break,result=tokio::time::timeout(Duration::from_secs(5),remote.window_change(cols,rows,0,0))=>result};
                    if !matches!(result,Ok(Ok(()))){failed=true;break;}
                },
                None=>break,
            },
            message=remote.wait(),if !waiting=>match message {
                Some(russh::ChannelMsg::Data{data})|Some(russh::ChannelMsg::ExtendedData{data,..})=>{
                    // Configured SSH packet limit is 32 KiB. Never buffer an unbounded message.
                    if data.len()>65536 {failed=true;break;}
                    sequence+=1;
                    if channel.send(TerminalEvent{terminal_id:id,session_id:session,generation,sequence,kind:"data".into(),bytes:data.to_vec(),exit_status:None}).is_err(){break;}
                    waiting=true;deadline=tokio::time::Instant::now()+Duration::from_secs(30);
                },
                Some(russh::ChannelMsg::ExitStatus{exit_status})=>{exit=Some(exit_status);},
                Some(russh::ChannelMsg::Close)|Some(russh::ChannelMsg::Eof)|None=>break,
                _=>{},
            }
        }
    }
    if cancel.is_cancelled() || failed {
        let _ = tokio::time::timeout(Duration::from_secs(1), remote.signal(russh::Sig::HUP)).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), remote.close()).await;
    sequence += 1;
    let _ = channel.send(TerminalEvent {
        terminal_id: id,
        session_id: session,
        generation,
        sequence,
        kind: if failed { "error" } else { "closed" }.into(),
        bytes: Vec::new(),
        exit_status: exit,
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_dimensions_and_quotes_directory() {
        assert!(dimensions(0, 24).is_err());
        assert!(dimensions(80, 24).is_ok());
        assert_eq!(quote_path("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }
}
