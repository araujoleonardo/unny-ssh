use crate::{
    ipc::{AppError, Result},
    services::profiles::{AuthMode, ProfileInput, Secret},
};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate},
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct Verifier {
    pub expected: Option<String>,
    pub observed: Arc<Mutex<Option<(String, String)>>>,
    pub unsupported: Arc<std::sync::atomic::AtomicBool>,
}
impl client::Handler for Verifier {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        if key.certificate().is_some() {
            self.unsupported
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return Ok(false);
        }
        let public = key.public_key();
        let wire = public.to_openssh().map_err(|_| russh::Error::UnknownKey)?;
        let fingerprint = public.fingerprint(HashAlg::Sha256).to_string();
        let accepted = self
            .expected
            .as_ref()
            .is_some_and(|expected| same_public_key(expected, &wire));
        if let Ok(mut observed) = self.observed.lock() {
            *observed = Some((wire, fingerprint));
        }
        Ok(accepted)
    }
}
pub type SshHandle = client::Handle<Verifier>;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub fn network() -> AppError {
    AppError::new(
        "CONNECTION_FAILED",
        "Não foi possível conectar. Confira endereço, rede/VPN e serviço SSH.",
    )
}
pub async fn authenticate(
    handle: &mut SshHandle,
    profile: &ProfileInput,
    secret: Option<Secret>,
) -> Result<()> {
    let success = match profile.auth_mode {
        AuthMode::Password => {
            let Some(Secret::Password { password }) = secret.as_ref() else {
                return Err(AppError::new(
                    "CREDENTIAL_REQUIRED",
                    "Informe uma senha para esta conexão.",
                ));
            };
            handle
                .authenticate_password(&profile.username, password.as_str())
                .await
                .map_err(|_| network())?
                .success()
        }
        AuthMode::Key => {
            let Some(Secret::Key {
                private_key,
                passphrase,
            }) = secret.as_ref()
            else {
                return Err(AppError::new(
                    "CREDENTIAL_REQUIRED",
                    "Importe a chave privada desta conexão.",
                ));
            };
            let private_key = zeroize::Zeroizing::new(private_key.clone());
            let passphrase = zeroize::Zeroizing::new(passphrase.clone());
            let key = tokio::task::spawn_blocking(move || {
                russh::keys::decode_secret_key(
                    &private_key,
                    if passphrase.is_empty() {
                        None
                    } else {
                        Some(&passphrase)
                    },
                )
            })
            .await
            .map_err(|_| network())?
            .map_err(|_| {
                AppError::new(
                    "INVALID_KEY",
                    "Não foi possível abrir a chave. Confira o formato e a passphrase.",
                )
            })?;
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .map_err(|_| network())?
                .flatten();
            if key.algorithm().is_rsa() && hash.is_none() {
                return Err(AppError::new(
                    "UNSUPPORTED",
                    "O servidor não oferece assinatura RSA SHA-2.",
                ));
            }
            handle
                .authenticate_publickey(
                    &profile.username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map_err(|_| network())?
                .success()
        }
        AuthMode::Agent => agent_auth(handle, &profile.username).await?,
    };
    if !success {
        return Err(AppError::new(
            "AUTH_FAILED",
            "Autenticação recusada. Confira usuário e credencial.",
        ));
    }
    Ok(())
}
async fn agent_auth(handle: &mut SshHandle, user: &str) -> Result<bool> {
    use russh::keys::agent::client::AgentClient;
    #[cfg(unix)]
    let mut agent = AgentClient::connect_env()
        .await
        .map_err(|_| agent_unavailable())?;
    #[cfg(windows)]
    let mut agent = AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|_| agent_unavailable())?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|_| agent_unavailable())?;
    if identities.is_empty() {
        return Err(AppError::new(
            "AGENT_EMPTY",
            "O SSH Agent não contém chaves. Adicione uma chave e tente novamente.",
        ));
    }
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|_| network())?
        .flatten();
    // A bounded selection avoids exhausting the server's authentication budget.
    for identity in identities.into_iter().take(5) {
        let key: PublicKey = identity.public_key().into_owned();
        if key.algorithm().is_rsa() && hash.is_none() {
            continue;
        }
        if handle
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
            .map_err(|_| agent_unavailable())?
            .success()
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn agent_unavailable() -> AppError {
    AppError::new(
        "AGENT_UNAVAILABLE",
        "SSH Agent indisponível. Inicie o Agent nativo e carregue sua chave.",
    )
}

pub fn same_public_key(left: &str, right: &str) -> bool {
    match (
        PublicKey::from_openssh(left),
        PublicKey::from_openssh(right),
    ) {
        (Ok(left), Ok(right)) => left.key_data() == right.key_data(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_key_comments_do_not_change_identity() {
        let key =
            russh::keys::PrivateKey::random(&mut rand10::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let wire = key.public_key().to_openssh().unwrap();
        assert!(same_public_key(
            &format!("{wire} fixture@one"),
            &format!("{wire} fixture@two")
        ));
        assert!(!same_public_key(&wire, "invalid public key"));
    }
}

// Cancelling a session wakes both halves of even a stalled TCP transport, so
// russh's background connection cannot outlive a disconnected/locked session.
struct CancellableSocket {
    socket: tokio::net::TcpStream,
    cancelled: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>,
    cancelled_observed: bool,
}
impl CancellableSocket {
    fn new(socket: tokio::net::TcpStream, cancel: tokio_util::sync::CancellationToken) -> Self {
        Self {
            socket,
            cancelled: Box::pin(cancel.cancelled_owned()),
            cancelled_observed: false,
        }
    }
    fn check(&mut self, cx: &mut std::task::Context<'_>) -> std::io::Result<()> {
        if self.cancelled_observed || self.cancelled.as_mut().poll(cx).is_ready() {
            self.cancelled_observed = true;
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
impl tokio::io::AsyncRead for CancellableSocket {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.check(cx)?;
        std::pin::Pin::new(&mut self.socket).poll_read(cx, buf)
    }
}
impl tokio::io::AsyncWrite for CancellableSocket {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.check(cx)?;
        std::pin::Pin::new(&mut self.socket).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.check(cx)?;
        std::pin::Pin::new(&mut self.socket).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.socket).poll_shutdown(cx)
    }
}
pub async fn connect(
    config: Arc<client::Config>,
    host: &str,
    port: u16,
    handler: Verifier,
    cancel: tokio_util::sync::CancellationToken,
) -> std::result::Result<SshHandle, russh::Error> {
    let socket = tokio::net::TcpStream::connect((host, port)).await?;
    client::connect_stream(config, CancellableSocket::new(socket, cancel), handler).await
}
