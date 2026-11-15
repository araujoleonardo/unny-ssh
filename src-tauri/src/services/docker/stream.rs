use super::*;
use tauri::ipc::Channel;
use tokio::{sync::mpsc, time::Instant as TokioInstant};
use tokio_util::sync::CancellationToken;
pub struct Job {
    pub session: Uuid,
    pub generation: u64,
    pub tx: mpsc::Sender<u64>,
    pub cancel: CancellationToken,
    pub follow: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub operation_id: Uuid,
    pub session_id: Uuid,
    pub generation: u64,
    pub sequence: u64,
    pub kind: String,
    pub bytes: Vec<u8>,
    pub exit_code: Option<u32>,
    pub message: Option<String>,
}
pub fn control(
    state: &AppState,
    session: Uuid,
    generation: u64,
    id: Uuid,
    sequence: Option<u64>,
) -> Result<()> {
    state.authorize(generation)?;
    let jobs = state.docker.lock().map_err(|_| AppError::io())?;
    let job = jobs.jobs.get(&id).ok_or_else(stale)?;
    if job.session != session || job.generation != generation {
        return Err(stale());
    }
    if let Some(n) = sequence {
        job.tx
            .try_send(n)
            .map_err(|_| AppError::new("BUSY", "ACK remoto fora do limite."))?;
    } else {
        job.cancel.cancel();
    }
    Ok(())
}
pub fn start(
    state: Arc<AppState>,
    session: Uuid,
    generation: u64,
    command: String,
    channel: Channel<Event>,
    follow: bool,
) -> Result<Uuid> {
    let cancel = remote::owner(&state, session, generation)?;
    let id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(4);
    {
        let mut d = state.docker.lock().map_err(|_| AppError::io())?;
        if d.jobs.len() >= 16 || d.jobs.values().filter(|j| j.session == session).count() >= 4 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Limite de streams Docker (4 por sessão / 16 no total).",
            ));
        }
        if !follow && d.jobs.values().any(|j| j.session == session && !j.follow) {
            return Err(AppError::new(
                "BUSY",
                "Já existe uma operação Compose nesta sessão.",
            ));
        }
        d.jobs.insert(
            id,
            Job {
                session,
                generation,
                tx,
                cancel: cancel.clone(),
                follow,
            },
        );
    }
    tauri::async_runtime::spawn(async move {
        let mut sequence = 0;
        let mut exit = None;
        let result=async{
 let mut remote=remote::open(&state,session,generation,command,&cancel).await?;
 let result=async{
 let deadline=TokioInstant::now()+Duration::from_secs(600);
 loop{
 let msg=tokio::select!{biased;_=cancel.cancelled()=>return Err(remote::interrupted()),_=tokio::time::sleep_until(deadline),if !follow=>return Err(AppError::new("TIMEOUT","Operação excedeu dez minutos; confira o estado remoto antes de repetir.")),m=remote.wait()=>m};
 let packet=match msg {
 Some(russh::ChannelMsg::Data{data})=>Some(("stdout",data.to_vec())),
 Some(russh::ChannelMsg::ExtendedData{data,..})=>Some(("stderr",data.to_vec())),
 Some(russh::ChannelMsg::ExitStatus{exit_status})=>{exit=Some(exit_status);None},
 Some(russh::ChannelMsg::Close)|None=>break,_=>None
 };
 if let Some((kind,bytes))=packet {
 if bytes.len()>65536{return Err(AppError::new("LIMIT_REACHED","Pacote de saída Docker excessivo."));}sequence+=1;
 channel.send(Event{operation_id:id,session_id:session,generation,sequence,kind:kind.into(),bytes,exit_code:None,message:None}).map_err(|_|remote::interrupted())?;
 let ack=tokio::select!{biased;_=cancel.cancelled()=>return Err(remote::interrupted()),v=tokio::time::timeout(Duration::from_secs(30),rx.recv())=>v.map_err(|_|remote::interrupted())?};if ack!=Some(sequence){return Err(remote::interrupted());}
 }
 }
 if exit==Some(0){Ok(())}else if exit.is_some(){Err(AppError::new("REMOTE_COMMAND_FAILED","O comando terminou com erro. Confira a saída; efeitos parciais podem permanecer."))}else{Err(remote::interrupted())}
 }.await;
 if result.is_err(){let _=tokio::time::timeout(Duration::from_secs(1),remote.signal(russh::Sig::TERM)).await;}
 let _=tokio::time::timeout(Duration::from_secs(1),remote.close()).await;result
 }.await;
        sequence += 1;
        let kind = if cancel.is_cancelled() {
            "cancelled"
        } else if result.is_ok() {
            "completed"
        } else {
            "failed"
        };
        let _ = channel.send(Event {
            operation_id: id,
            session_id: session,
            generation,
            sequence,
            kind: kind.into(),
            bytes: vec![],
            exit_code: exit,
            message: result.err().map(|e| e.message.to_owned()),
        });
        if let Ok(mut d) = state.docker.lock() {
            d.jobs.remove(&id);
        }
    });
    Ok(id)
}
