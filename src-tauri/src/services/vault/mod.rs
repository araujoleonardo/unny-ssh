use crate::{
    adapters::storage::Storage,
    ipc::{AppError, Result},
};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    generation: u64,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: [u8; 16],
    nonce: [u8; 24],
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    header: Header,
    ciphertext: Vec<u8>,
}
// Sensitive metadata and the draft index share the publication boundary.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VaultPayload {
    pub(crate) profiles: Vec<serde_json::Value>,
    pub(crate) secrets: Vec<serde_json::Value>,
    pub(crate) known_hosts: Vec<serde_json::Value>,
    pub(super) draft_index: Vec<DraftRef>,
    #[serde(default)]
    pub(crate) compose_projects: Vec<serde_json::Value>,
}
impl Drop for VaultPayload {
    fn drop(&mut self) {
        fn clear(value: &mut serde_json::Value) {
            use zeroize::Zeroize;
            match value {
                serde_json::Value::String(s) => s.zeroize(),
                serde_json::Value::Array(items) => items.iter_mut().for_each(clear),
                serde_json::Value::Object(map) => map.values_mut().for_each(clear),
                _ => {}
            }
        }
        for value in self
            .profiles
            .iter_mut()
            .chain(self.secrets.iter_mut())
            .chain(self.known_hosts.iter_mut())
            .chain(self.compose_projects.iter_mut())
        {
            clear(value);
        }
    }
}
#[derive(Serialize, Deserialize)]
pub(super) struct DraftRef {
    id: uuid::Uuid,
    generation: u64,
    blob: uuid::Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub state: &'static str,
    pub generation: u64,
}
struct OpenVault {
    key: Zeroizing<[u8; 32]>,
    plaintext: Zeroizing<Vec<u8>>,
    header: Header,
}
pub struct Vault {
    storage: Storage,
    open: Option<OpenVault>,
    generation: u64,
    pending: Option<Vec<drafts::Draft>>,
}
fn invalid() -> AppError {
    AppError::new(
        "INVALID_VAULT",
        "Cofre inválido ou incompatível. Preserve os arquivos antes de redefinir.",
    )
}
fn auth() -> AppError {
    AppError::new(
        "AUTH_FAILED",
        "Senha incorreta ou cofre adulterado. Nenhum dado foi alterado.",
    )
}
fn validate(h: &Header) -> Result<()> {
    if h.version != 1
        || h.memory_kib != 65536
        || h.iterations != 3
        || h.parallelism != 1
        || h.generation == 0
    {
        return Err(invalid());
    }
    Ok(())
}
fn derive(password: &str, header: &Header) -> Result<Zeroizing<[u8; 32]>> {
    validate(header)?;
    if password.is_empty() || password.len() > 1024 {
        return Err(AppError::invalid());
    }
    let mut key = Zeroizing::new([0; 32]);
    let params = Params::new(
        header.memory_kib,
        header.iterations,
        header.parallelism,
        Some(32),
    )
    .map_err(|_| invalid())?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &header.salt, key.as_mut())
        .map_err(|_| invalid())?;
    Ok(key)
}
fn encode(open: &OpenVault) -> Result<Vec<u8>> {
    let aad = serde_json::to_vec(&open.header).map_err(|_| invalid())?;
    let ciphertext = XChaCha20Poly1305::new_from_slice(open.key.as_ref())
        .map_err(|_| invalid())?
        .encrypt(
            XNonce::from_slice(&open.header.nonce),
            Payload {
                msg: &open.plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| invalid())?;
    serde_json::to_vec(&Envelope {
        header: open.header.clone(),
        ciphertext,
    })
    .map_err(|_| invalid())
}
fn decode(bytes: &[u8], password: &str) -> Result<OpenVault> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let key = derive(password, &envelope.header)?;
    let aad = serde_json::to_vec(&envelope.header).map_err(|_| invalid())?;
    let plaintext = Zeroizing::new(
        XChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| invalid())?
            .decrypt(
                XNonce::from_slice(&envelope.header.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| auth())?,
    );
    let _: VaultPayload = serde_json::from_slice(&plaintext).map_err(|_| invalid())?;
    Ok(OpenVault {
        key,
        plaintext,
        header: envelope.header,
    })
}
fn header(generation: u64) -> Header {
    let mut h = Header {
        version: 1,
        generation,
        memory_kib: 65536,
        iterations: 3,
        parallelism: 1,
        salt: [0; 16],
        nonce: [0; 24],
    };
    OsRng.fill_bytes(&mut h.salt);
    OsRng.fill_bytes(&mut h.nonce);
    h
}
impl Vault {
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            open: None,
            generation: 1,
            pending: None,
        }
    }
    pub fn audit(&self, operation: &'static str, result: &'static str, duration_ms: u128) {
        self.storage.audit(operation, result, duration_ms);
    }
    pub fn status(&self) -> VaultStatus {
        VaultStatus {
            state: if self.pending.is_some() {
                "lock_failed"
            } else if self.open.is_some() {
                "unlocked"
            } else if self.storage.exists() {
                "locked"
            } else {
                "missing"
            },
            generation: self.generation,
        }
    }
    pub fn authorize(&self, generation: u64) -> Result<()> {
        if self.open.is_none() || self.pending.is_some() {
            return Err(AppError::new(
                "VAULT_LOCKED",
                "Desbloqueie o cofre para continuar.",
            ));
        }
        if generation != self.generation {
            return Err(AppError::new("STALE_GENERATION", "Esta operação expirou."));
        }
        Ok(())
    }
    pub(crate) fn payload(&self) -> Result<VaultPayload> {
        let open = self
            .open
            .as_ref()
            .ok_or_else(|| AppError::new("VAULT_LOCKED", "Desbloqueie o cofre."))?;
        serde_json::from_slice(&open.plaintext).map_err(|_| invalid())
    }
    pub(crate) fn transaction<T>(
        &mut self,
        generation: u64,
        update: impl FnOnce(&mut VaultPayload) -> Result<T>,
    ) -> Result<T> {
        self.authorize(generation)?;
        let mut payload = self.payload()?;
        let result = update(&mut payload)?;
        self.publish_payload(&payload)?;
        Ok(result)
    }
    fn publish_payload(&mut self, payload: &VaultPayload) -> Result<()> {
        let current = self.open.as_ref().ok_or_else(invalid)?;
        let mut header = current.header.clone();
        header.generation = header.generation.checked_add(1).ok_or_else(invalid)?;
        OsRng.fill_bytes(&mut header.nonce);
        let next = OpenVault {
            header,
            key: current.key.clone(),
            plaintext: Zeroizing::new(serde_json::to_vec(&payload).map_err(|_| invalid())?),
        };
        let bytes = encode(&next)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "O cofre atingiu o limite de tamanho.",
            ));
        }
        self.storage.publish(&bytes)?;
        self.open = Some(next);
        self.storage.cleanup_blobs(
            &payload
                .draft_index
                .iter()
                .map(|r| r.blob)
                .collect::<Vec<_>>(),
        );
        Ok(())
    }
    pub fn create(&mut self, password: &str) -> Result<VaultStatus> {
        if self.storage.exists() {
            return Err(AppError::new(
                "ALREADY_EXISTS",
                "O cofre existente deve ser desbloqueado ou redefinido explicitamente.",
            ));
        }
        if password.chars().count() < 12 {
            return Err(AppError::new(
                "WEAK_PASSWORD",
                "Use pelo menos 12 caracteres.",
            ));
        }
        let header = header(1);
        let key = derive(password, &header)?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(&VaultPayload::default()).map_err(|_| invalid())?);
        let open = OpenVault {
            key,
            plaintext,
            header,
        };
        let bytes = encode(&open)?;
        decode(&bytes, password)?;
        self.storage.publish(&bytes)?;
        self.open = Some(open);
        Ok(self.status())
    }
    pub fn unlock(&mut self, password: &str) -> Result<VaultStatus> {
        if self.pending.is_some() {
            return Err(AppError::new(
                "LOCK_FAILED",
                "Autentique para recuperar o bloqueio incompleto.",
            ));
        }
        let open = decode(&self.storage.read()?, password)?;
        self.open = Some(open);
        Ok(self.status())
    }
    pub fn lock(&mut self) -> VaultStatus {
        if self.pending.is_some() {
            return self.status();
        }
        self.open = None;
        self.generation += 1;
        self.status()
    }
    pub fn change_password(&mut self, old: &str, new: &str) -> Result<VaultStatus> {
        self.authorize(self.generation)?;
        if new.chars().count() < 12 {
            return Err(AppError::new(
                "WEAK_PASSWORD",
                "Use pelo menos 12 caracteres.",
            ));
        }
        let current = decode(&self.storage.read()?, old)?;
        let header = header(
            current
                .header
                .generation
                .checked_add(1)
                .ok_or_else(invalid)?,
        );
        let key = derive(new, &header)?;
        let mut payload: VaultPayload =
            serde_json::from_slice(&current.plaintext).map_err(|_| invalid())?;
        let old_refs = payload
            .draft_index
            .iter()
            .map(|r| r.blob)
            .collect::<Vec<_>>();
        for reference in &mut payload.draft_index {
            let draft = drafts::read(&self.storage, &current.key, reference)?;
            *reference = drafts::write(&self.storage, &key, header.generation, &draft)?;
        }
        let next = OpenVault {
            key,
            plaintext: Zeroizing::new(serde_json::to_vec(&payload).map_err(|_| invalid())?),
            header,
        };
        let bytes = encode(&next)?;
        decode(&bytes, new)?;
        // Old generation remains the published file until replacement succeeds.
        self.storage.publish(&bytes)?;
        self.open = Some(next);
        self.storage.cleanup_blobs(
            &payload
                .draft_index
                .iter()
                .map(|r| r.blob)
                .collect::<Vec<_>>(),
        );
        for id in old_refs {
            self.storage.remove_blob(id);
        }
        self.generation += 1;
        Ok(self.status())
    }
    pub fn reset(&mut self, password: &str, confirmation: &str) -> Result<VaultStatus> {
        if confirmation != "APAGAR MEU COFRE" {
            return Err(AppError::invalid());
        }
        if password.chars().count() < 12 {
            return Err(AppError::new(
                "WEAK_PASSWORD",
                "Use pelo menos 12 caracteres.",
            ));
        }
        let h = header(1);
        let next = OpenVault {
            key: derive(password, &h)?,
            plaintext: Zeroizing::new(
                serde_json::to_vec(&VaultPayload::default()).map_err(|_| invalid())?,
            ),
            header: h,
        };
        let bytes = encode(&next)?;
        decode(&bytes, password)?;
        self.storage.publish(&bytes)?;
        self.open = Some(next);
        self.pending = None;
        self.storage.cleanup_blobs(&[]);
        self.generation += 1;
        Ok(self.status())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
        (dir, v)
    }
    #[test]
    fn lifecycle_and_rotation() {
        let (_dir, mut v) = vault();
        assert!(v.authorize(1).is_err());
        v.create("a long master password").unwrap();
        let payload = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({
                "profiles":[{"id":"fixture-profile","host":"example.invalid"}],
                "secrets":[{"id":"fixture-secret","value":"synthetic-only"}],
                "known_hosts":[], "draft_index":[], "compose_projects":[]
            }))
            .unwrap(),
        );
        v.open.as_mut().unwrap().plaintext = payload.clone();
        v.storage
            .publish(&encode(v.open.as_ref().unwrap()).unwrap())
            .unwrap();
        let before = v.storage.read().unwrap();
        assert!(!String::from_utf8_lossy(&before).contains("synthetic-only"));
        v.change_password("a long master password", "another long password")
            .unwrap();
        assert_ne!(before, v.storage.read().unwrap());
        v.lock();
        assert!(v.unlock("a long master password").is_err());
        v.unlock("another long password").unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&v.open.as_ref().unwrap().plaintext)
                .unwrap(),
            serde_json::from_slice::<serde_json::Value>(&payload).unwrap()
        );
        assert_eq!(v.authorize(1).unwrap_err().code, "STALE_GENERATION");
    }
    #[test]
    fn corruption_and_kdf_bounds_preserve_disk() {
        let (_dir, mut v) = vault();
        v.create("a long master password").unwrap();
        let bytes = v.storage.read().unwrap();
        let mut e: Envelope = serde_json::from_slice(&bytes).unwrap();
        e.header.memory_kib = u32::MAX;
        assert_eq!(
            decode(&serde_json::to_vec(&e).unwrap(), "x")
                .err()
                .unwrap()
                .code,
            "INVALID_VAULT"
        );
        e = serde_json::from_slice(&bytes).unwrap();
        e.header.generation += 1;
        assert_eq!(
            decode(&serde_json::to_vec(&e).unwrap(), "a long master password")
                .err()
                .unwrap()
                .code,
            "AUTH_FAILED"
        );
        e = serde_json::from_slice(&bytes).unwrap();
        e.ciphertext[0] ^= 1;
        assert!(decode(&serde_json::to_vec(&e).unwrap(), "a long master password").is_err());
        assert!(v.change_password("wrong", "another long password").is_err());
        assert_eq!(bytes, v.storage.read().unwrap());
    }
    #[test]
    fn lock_and_reset_confirmation() {
        let (dir, mut v) = vault();
        assert!(Storage::open(dir.path().join("vault")).is_err());
        v.create("a long master password").unwrap();
        let bytes = v.storage.read().unwrap();
        assert!(v.reset("another long password", "yes").is_err());
        assert_eq!(bytes, v.storage.read().unwrap());
        v.reset("another long password", "APAGAR MEU COFRE")
            .unwrap();
        v.lock();
        v.unlock("another long password").unwrap();
    }
    #[test]
    fn invalid_vault_never_recreated() {
        let (_dir, mut vault) = vault();
        vault.storage.publish(b"invalid envelope").unwrap();
        assert!(vault.create("a long master password").is_err());
        assert!(vault.unlock("a long master password").is_err());
        assert_eq!(vault.storage.read().unwrap(), b"invalid envelope");
    }
    #[test]
    fn nonce_and_permissions() {
        let (dir, mut v) = vault();
        v.create("a long master password").unwrap();
        let a: Envelope = serde_json::from_slice(&v.storage.read().unwrap()).unwrap();
        v.change_password("a long master password", "a long master password")
            .unwrap();
        let b: Envelope = serde_json::from_slice(&v.storage.read().unwrap()).unwrap();
        assert_ne!(a.header.nonce, b.header.nonce);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.path().join("vault/vault.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    #[ignore]
    fn benchmark_kdf() {
        let now = std::time::Instant::now();
        derive("benchmark only", &header(1)).unwrap();
        println!("Argon2id 64MiB t=3 p=1: {:?}", now.elapsed());
    }
}

#[cfg(test)]
mod native_fixture {
    #[test]
    #[ignore]
    fn seed() {
        let dir =
            std::env::var_os("UNNY_SMOKE_DATA_DIR").expect("isolated smoke directory required");
        let mut vault =
            super::Vault::new(crate::adapters::storage::Storage::open(dir.into()).unwrap());
        // Synthetic test credential. Never used for a user vault or remote service.
        vault.create("unny disposable smoke fixture").unwrap();
    }
}

pub mod drafts;
