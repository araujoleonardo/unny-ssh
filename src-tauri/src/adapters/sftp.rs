//! SFTP v3 framing with byte-preserving paths/handles. russh-sftp 3.0's public
//! path/name APIs use String; reuse its attributes/flags while retaining bytes.
use crate::ipc::{AppError, Result};
use russh_sftp::protocol::FileAttributes;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
pub const CHUNK: usize = 65536;
const MAX_PACKET: usize = 1024 * 1024;
pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub struct Wire {
    io: Box<dyn Io>,
    next: u32,
    pub atomic_rename: bool,
    pub fsync: bool,
    pub broken: bool,
    cancel: CancellationToken,
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: Vec<u8>,
    pub attrs: FileAttributes,
}
pub fn protocol() -> AppError {
    AppError::new(
        "SFTP_PROTOCOL",
        "O servidor retornou dados SFTP inválidos ou excedeu os limites.",
    )
}
pub fn interrupted() -> AppError {
    AppError::new(
        "INTERRUPTED",
        "Operação interrompida. O resultado pode estar incompleto.",
    )
}
pub fn status(code: u32) -> AppError {
    match code {
        1 => AppError::new("EOF", "Fim da listagem ou do arquivo."),
        2 => AppError::new("NOT_FOUND", "O arquivo ou diretório não existe mais."),
        3 => AppError::new("PERMISSION_DENIED", "Permissão negada pelo servidor."),
        8 => AppError::new("UNSUPPORTED", "O servidor não oferece esta operação SFTP."),
        _ => AppError::new(
            "SFTP_FAILURE",
            "O servidor recusou a operação. Confira permissões, colisões e se a pasta está vazia.",
        ),
    }
}
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(protocol)?;
        let value = self.bytes.get(self.pos..end).ok_or_else(protocol)?;
        self.pos = end;
        Ok(value)
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().map_err(|_| protocol())?,
        ))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().map_err(|_| protocol())?,
        ))
    }
    pub fn string(&mut self) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        if n > MAX_PACKET {
            return Err(protocol());
        }
        Ok(self.take(n)?.to_vec())
    }
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn attrs(&mut self) -> Result<FileAttributes> {
        let flags = self.u32()?;
        if flags & !0x8000000f != 0 {
            return Err(protocol());
        }
        let mut attrs = FileAttributes::default();
        if flags & 1 != 0 {
            attrs.size = Some(self.u64()?);
        }
        if flags & 2 != 0 {
            attrs.uid = Some(self.u32()?);
            attrs.gid = Some(self.u32()?);
        }
        if flags & 4 != 0 {
            attrs.permissions = Some(self.u32()?);
        }
        if flags & 8 != 0 {
            attrs.atime = Some(self.u32()?);
            attrs.mtime = Some(self.u32()?);
        }
        if flags & 0x80000000 != 0 {
            let count = self.u32()?;
            if count > 1024 {
                return Err(protocol());
            }
            for _ in 0..count {
                self.string()?;
                self.string()?;
            }
        }
        Ok(attrs)
    }
}
pub fn string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}
fn attrs_bytes(attrs: &FileAttributes) -> Result<Vec<u8>> {
    Ok(russh_sftp::ser::to_bytes(attrs)
        .map_err(|_| protocol())?
        .to_vec())
}
impl Wire {
    pub async fn new(io: impl Io + 'static, cancel: CancellationToken) -> Result<Self> {
        let mut wire = Self {
            io: Box::new(io),
            next: 0,
            atomic_rename: false,
            fsync: false,
            broken: false,
            cancel,
        };
        let packet = wire.exchange(1, 3u32.to_be_bytes().to_vec()).await?;
        if packet.first() != Some(&2) {
            return Err(protocol());
        }
        let mut reader = Reader::new(&packet[1..]);
        if reader.u32()? != 3 {
            return Err(AppError::new(
                "UNSUPPORTED",
                "Esta versão do SFTP não é suportada (necessário v3).",
            ));
        }
        let mut count = 0;
        while reader.remaining() > 0 {
            count += 1;
            if count > 256 {
                return Err(protocol());
            }
            let name = reader.string()?;
            let value = reader.string()?;
            if name == b"posix-rename@openssh.com" && value == b"1" {
                wire.atomic_rename = true;
            }
            if name == b"fsync@openssh.com" && value == b"1" {
                wire.fsync = true;
            }
        }
        Ok(wire)
    }
    async fn exchange(&mut self, kind: u8, payload: Vec<u8>) -> Result<Vec<u8>> {
        if self.broken {
            return Err(interrupted());
        }
        if payload.len() > MAX_PACKET - 1 {
            return Err(protocol());
        }
        let operation = async {
            self.io
                .write_all(&((payload.len() + 1) as u32).to_be_bytes())
                .await?;
            self.io.write_all(&[kind]).await?;
            self.io.write_all(&payload).await?;
            self.io.flush().await?;
            let len = self.io.read_u32().await? as usize;
            if !(1..=MAX_PACKET).contains(&len) {
                return Err(std::io::Error::other("invalid packet"));
            }
            let mut response = vec![0; len];
            self.io.read_exact(&mut response).await?;
            Ok::<_, std::io::Error>(response)
        };
        let result = tokio::select! {biased;_=self.cancel.cancelled()=>Err(interrupted()),result=tokio::time::timeout(Duration::from_secs(30),operation)=>match result{Ok(Ok(packet))=>Ok(packet),Ok(Err(_))=>Err(AppError::new("CONNECTION_LOST","O canal SFTP foi encerrado.")),Err(_)=>Err(AppError::new("TIMEOUT","O servidor SFTP não respondeu em 30 segundos."))}};
        if result.is_err() {
            self.broken = true;
        }
        result
    }
    async fn call(&mut self, kind: u8, body: Vec<u8>) -> Result<(u8, Vec<u8>)> {
        self.next = self.next.checked_add(1).ok_or_else(protocol)?;
        let id = self.next;
        let mut payload = id.to_be_bytes().to_vec();
        payload.extend(body);
        let packet = self.exchange(kind, payload).await?;
        let response = *packet.first().ok_or_else(protocol)?;
        let mut reader = Reader::new(&packet[1..]);
        if reader.u32()? != id {
            self.broken = true;
            return Err(protocol());
        }
        if response == 101 {
            let code = reader.u32()?;
            if code != 0 {
                return Err(status(code));
            }
        }
        Ok((response, packet[5..].to_vec()))
    }
    async fn expected(&mut self, kind: u8, body: Vec<u8>, response: u8) -> Result<Vec<u8>> {
        let (actual, data) = self.call(kind, body).await?;
        if actual != response {
            self.broken = true;
            return Err(protocol());
        }
        Ok(data)
    }
    async fn path_call(&mut self, kind: u8, path: &[u8], response: u8) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        string(&mut body, path);
        self.expected(kind, body, response).await
    }
    fn names(bytes: &[u8]) -> Result<Vec<Entry>> {
        let mut reader = Reader::new(bytes);
        let count = reader.u32()? as usize;
        if count > 8192 || count > reader.remaining() / 12 {
            return Err(protocol());
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let name = reader.string()?;
            reader.string()?;
            let attrs = reader.attrs()?;
            entries.push(Entry { name, attrs });
        }
        if reader.remaining() != 0 {
            return Err(protocol());
        }
        Ok(entries)
    }
    pub async fn realpath(&mut self, path: &[u8]) -> Result<Vec<u8>> {
        let bytes = self.path_call(16, path, 104).await?;
        let entries = Self::names(&bytes)?;
        if entries.len() != 1 {
            return Err(protocol());
        }
        let path = entries[0].name.clone();
        if path.len() > 16384 || path.contains(&0) || !path.starts_with(b"/") {
            return Err(protocol());
        }
        Ok(path)
    }
    pub async fn stat(&mut self, path: &[u8], follow: bool) -> Result<FileAttributes> {
        let bytes = self
            .path_call(if follow { 17 } else { 7 }, path, 105)
            .await?;
        Reader::new(&bytes).attrs()
    }
    pub async fn maybe_stat(&mut self, path: &[u8]) -> Result<Option<FileAttributes>> {
        match self.stat(path, false).await {
            Ok(attrs) => Ok(Some(attrs)),
            Err(error) if error.code == "NOT_FOUND" => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub async fn fstat(&mut self, handle: &[u8]) -> Result<FileAttributes> {
        let bytes = self.path_call(8, handle, 105).await?;
        Reader::new(&bytes).attrs()
    }
    pub async fn opendir(&mut self, path: &[u8]) -> Result<Vec<u8>> {
        let bytes = self.path_call(11, path, 102).await?;
        Reader::new(&bytes).string()
    }
    pub async fn readdir(&mut self, handle: &[u8]) -> Result<Option<Vec<Entry>>> {
        match self.path_call(12, handle, 104).await {
            Ok(bytes) => Ok(Some(Self::names(&bytes)?)),
            Err(error) if error.code == "EOF" => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub async fn close(&mut self, handle: &[u8]) -> Result<()> {
        self.path_call(4, handle, 101).await?;
        Ok(())
    }
    pub async fn open(
        &mut self,
        path: &[u8],
        flags: u32,
        permissions: Option<u32>,
    ) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        string(&mut body, path);
        body.extend(flags.to_be_bytes());
        body.extend(attrs_bytes(&FileAttributes {
            permissions,
            ..Default::default()
        })?);
        let bytes = self.expected(3, body, 102).await?;
        Reader::new(&bytes).string()
    }
    pub async fn read(&mut self, handle: &[u8], offset: u64) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        string(&mut body, handle);
        body.extend(offset.to_be_bytes());
        body.extend((CHUNK as u32).to_be_bytes());
        match self.expected(5, body, 103).await {
            Ok(bytes) => {
                let data = Reader::new(&bytes).string()?;
                if data.len() > CHUNK {
                    return Err(protocol());
                }
                Ok(data)
            }
            Err(e) if e.code == "EOF" => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }
    pub async fn write(&mut self, handle: &[u8], offset: u64, bytes: &[u8]) -> Result<()> {
        if bytes.len() > CHUNK {
            return Err(protocol());
        }
        let mut body = Vec::new();
        string(&mut body, handle);
        body.extend(offset.to_be_bytes());
        string(&mut body, bytes);
        self.expected(6, body, 101).await?;
        Ok(())
    }
    pub async fn set_permissions(&mut self, handle: &[u8], mode: u32) -> Result<()> {
        let mut body = Vec::new();
        string(&mut body, handle);
        body.extend(attrs_bytes(&FileAttributes {
            permissions: Some(mode & 0o7777),
            ..Default::default()
        })?);
        self.expected(10, body, 101).await?;
        Ok(())
    }
    pub async fn sync(&mut self, handle: &[u8]) -> Result<()> {
        if self.fsync {
            let mut body = Vec::new();
            string(&mut body, b"fsync@openssh.com");
            string(&mut body, handle);
            self.expected(200, body, 101).await?;
        }
        Ok(())
    }
    pub async fn mkdir(&mut self, path: &[u8]) -> Result<()> {
        let mut body = Vec::new();
        string(&mut body, path);
        body.extend(attrs_bytes(&FileAttributes {
            permissions: Some(0o755),
            ..Default::default()
        })?);
        self.expected(14, body, 101).await?;
        Ok(())
    }
    pub async fn remove(&mut self, path: &[u8], directory: bool) -> Result<()> {
        self.path_call(if directory { 15 } else { 13 }, path, 101)
            .await?;
        Ok(())
    }
    pub async fn rename(&mut self, old: &[u8], new: &[u8], replace: bool) -> Result<()> {
        let mut body = Vec::new();
        if replace {
            if !self.atomic_rename {
                return Err(status(8));
            }
            string(&mut body, b"posix-rename@openssh.com");
        }
        string(&mut body, old);
        string(&mut body, new);
        self.expected(if replace { 200 } else { 18 }, body, 101)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn receive(io: &mut tokio::io::DuplexStream) -> Vec<u8> {
        let len = io.read_u32().await.unwrap();
        let mut bytes = vec![0; len as usize];
        io.read_exact(&mut bytes).await.unwrap();
        bytes
    }
    async fn send(io: &mut tokio::io::DuplexStream, bytes: &[u8]) {
        io.write_u32(bytes.len() as u32).await.unwrap();
        io.write_all(bytes).await.unwrap();
    }
    #[tokio::test]
    async fn non_utf8_paths_names_and_handles_survive_the_wire() {
        let (client, mut server) = tokio::io::duplex(4096);
        let task = tokio::spawn(async move {
            assert_eq!(receive(&mut server).await, [1, 0, 0, 0, 3]);
            send(&mut server, &[2, 0, 0, 0, 3]).await;
            let request = receive(&mut server).await;
            assert_eq!(request[0], 11);
            let mut r = Reader::new(&request[5..]);
            assert_eq!(r.string().unwrap(), b"/bad_\xff");
            let mut reply = vec![102];
            reply.extend(&request[1..5]);
            string(&mut reply, b"handle_\xfe");
            send(&mut server, &reply).await;
            let request = receive(&mut server).await;
            assert_eq!(request[0], 12);
            assert_eq!(Reader::new(&request[5..]).string().unwrap(), b"handle_\xfe");
            let mut reply = vec![104];
            reply.extend(&request[1..5]);
            reply.extend(1u32.to_be_bytes());
            string(&mut reply, b"file_\xff");
            string(&mut reply, b"");
            reply.extend(4u32.to_be_bytes());
            reply.extend(0o100640u32.to_be_bytes());
            send(&mut server, &reply).await;
        });
        let mut wire = Wire::new(client, CancellationToken::new()).await.unwrap();
        let handle = wire.opendir(b"/bad_\xff").await.unwrap();
        assert_eq!(
            wire.readdir(&handle).await.unwrap().unwrap()[0].name,
            b"file_\xff"
        );
        task.await.unwrap();
    }
    #[tokio::test]
    async fn oversized_packet_and_cancel_are_bounded() {
        let (client, mut server) = tokio::io::duplex(64);
        let task = tokio::spawn(async move {
            receive(&mut server).await;
            server.write_u32((MAX_PACKET + 1) as u32).await.unwrap();
        });
        assert!(Wire::new(client, CancellationToken::new()).await.is_err());
        task.await.unwrap();
        let (client, _server) = tokio::io::duplex(64);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(matches!(Wire::new(client,cancel).await,Err(e) if e.code=="INTERRUPTED"));
    }
}
