//! Restricted command construction is separate from the SSH transport.
use crate::{
    commands::AppState,
    ipc::{AppError, Result},
    services::sessions::SessionCommand,
};
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub fn quote(value: &str) -> Result<String> {
    if value.contains('\0') || value.len() > 16384 {
        return Err(AppError::invalid());
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}
pub fn command(args: &[&str], directory: Option<&str>) -> Result<String> {
    let args = args
        .iter()
        .map(|a| quote(a))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(match directory {
        Some(path) => format!("cd -- {} && {args}", quote(path)?),
        None => args,
    })
}
pub fn docker(args: &[&str], directory: Option<&str>) -> Result<String> {
    let mut all = vec![
        "env",
        "-u",
        "DOCKER_HOST",
        "-u",
        "DOCKER_CONTEXT",
        "-u",
        "DOCKER_TLS_VERIFY",
        "-u",
        "DOCKER_CERT_PATH",
        "-u",
        "DOCKER_TLS",
        "-u",
        "COMPOSE_FILE",
        "-u",
        "COMPOSE_PROJECT_NAME",
        "-u",
        "COMPOSE_PROFILES",
        "LC_ALL=C",
        "docker",
        "--host",
        "unix:///var/run/docker.sock",
    ];
    all.extend(args);
    command(&all, directory)
}
pub fn owner(state: &AppState, session: Uuid, generation: u64) -> Result<CancellationToken> {
    state.authorize(generation)?;
    let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
    let entry = sessions.authorize(session, generation)?;
    if entry.info.state != "connected" || entry.cancel.is_cancelled() {
        return Err(AppError::new(
            "CONNECTION_LOST",
            "Conecte novamente manualmente. Comandos anteriores não serão repetidos.",
        ));
    }
    Ok(entry.cancel.child_token())
}
pub async fn open(
    state: &Arc<AppState>,
    session: Uuid,
    generation: u64,
    command: String,
    cancel: &CancellationToken,
) -> Result<russh::Channel<russh::client::Msg>> {
    owner(state, session, generation)?;
    let tx = {
        let sessions = state.sessions.lock().map_err(|_| AppError::io())?;
        sessions.authorize(session, generation)?.tx.clone()
    };
    let (reply, wait) = tokio::sync::oneshot::channel();
    tx.try_send(SessionCommand::OpenExec { command, reply })
        .map_err(|_| AppError::new("BUSY", "Aguarde a sessão processar os comandos anteriores."))?;
    tokio::select! {biased;_=cancel.cancelled()=>Err(interrupted()),result=tokio::time::timeout(Duration::from_secs(20),wait)=>result.map_err(|_|AppError::new("TIMEOUT","O servidor não abriu o canal de execução.")).and_then(|r|r.map_err(|_|interrupted())).and_then(|r|r)}
}
pub fn interrupted() -> AppError {
    AppError::new(
        "INTERRUPTED",
        "Operação interrompida. Efeitos já executados podem permanecer no servidor; confira antes de tentar novamente.",
    )
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u32,
}
impl Output {
    pub fn success(&self) -> Result<()> {
        if self.exit_code == 0 {
            Ok(())
        } else {
            Err(classify(self))
        }
    }
}
pub fn classify(result: &Output) -> AppError {
    let text = result.stderr.to_ascii_lowercase();
    if result.exit_code == 127 {
        AppError::new(
            "DOCKER_UNAVAILABLE",
            "Docker ou Compose não está instalado no servidor.",
        )
    } else if text.contains("permission denied") || text.contains("access is denied") {
        AppError::new(
            "PERMISSION_DENIED",
            "Sem permissão para acessar Docker neste servidor. Nenhum sudo foi executado.",
        )
    } else if text.contains("cannot connect") || text.contains("is the docker daemon running") {
        AppError::new(
            "DOCKER_UNAVAILABLE",
            "O daemon Docker local do servidor não está disponível.",
        )
    } else {
        AppError::new(
            "REMOTE_COMMAND_FAILED",
            "O comando remoto falhou. Confira o recurso, dependências e permissões.",
        )
    }
}
struct Active {
    state: Arc<AppState>,
    id: Uuid,
}
impl Drop for Active {
    fn drop(&mut self) {
        if let Ok(mut d) = self.state.docker.lock() {
            d.collects.remove(&self.id);
        }
    }
}
pub async fn collect(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    command: String,
) -> Result<Output> {
    let cancel = owner(&state, session, generation)?;
    let ctx = crate::services::docker::context(&state, session, generation)?;
    let _slot = tokio::select! {_=cancel.cancelled()=>return Err(interrupted()),slot=ctx.slots.acquire()=>slot.map_err(|_|interrupted())?};
    let id = Uuid::new_v4();
    {
        let mut d = state.docker.lock().map_err(|_| AppError::io())?;
        if d.collects.len() >= 64 {
            return Err(AppError::new(
                "BUSY",
                "Limite de consultas Docker atingido.",
            ));
        }
        d.collects.insert(id, (session, cancel.clone()));
    }
    let _active = Active {
        state: state.clone(),
        id,
    };
    let mut channel = open(&state, session, generation, command, &cancel).await?;
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err(interrupted()),result=tokio::time::timeout(Duration::from_secs(60),async{
        let(mut out,mut err,mut exit)=(Vec::new(),Vec::new(),None);
        while let Some(message)=channel.wait().await {match message {
            russh::ChannelMsg::Data{data}=>{if out.len()+err.len()+data.len()>8*1024*1024{return Err(AppError::new("LIMIT_REACHED","A resposta excede 8 MiB. Restrinja a consulta."));}out.extend_from_slice(&data);},
            russh::ChannelMsg::ExtendedData{data,..}=>{if out.len()+err.len()+data.len()>8*1024*1024{return Err(AppError::new("LIMIT_REACHED","A resposta excede 8 MiB."));}err.extend_from_slice(&data);},
            russh::ChannelMsg::ExitStatus{exit_status}=>exit=Some(exit_status),russh::ChannelMsg::Close=>break,_=>{}
        }}
        Ok(Output{stdout:String::from_utf8_lossy(&out).into_owned(),stderr:String::from_utf8_lossy(&err).into_owned(),exit_code:exit.ok_or_else(interrupted)?})
    })=>result.map_err(|_|AppError::new("TIMEOUT","O comando excedeu 60 segundos; confira efeitos antes de repetir.")).and_then(|r|r)};
    if result.is_err() {
        let _ =
            tokio::time::timeout(Duration::from_secs(1), channel.signal(russh::Sig::TERM)).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(1), channel.close()).await;
    owner(&state, session, generation)?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_is_structural() {
        assert_eq!(quote("a'b;$(id)").unwrap(), "'a'\\''b;$(id)'");
        assert!(quote("x\0y").is_err());
        assert_eq!(
            command(&["docker", "rm", "--", "a b"], Some("/a'b")).unwrap(),
            "cd -- '/a'\\''b' && 'docker' 'rm' '--' 'a b'"
        );
    }
}
