use crate::{
    ipc::{AppError, Result},
    services::vault::Vault,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Password,
    Key,
    Agent,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_mode: AuthMode,
    pub initial_directory: Option<String>,
}
impl ProfileInput {
    pub fn validate(&mut self) -> Result<()> {
        self.name = self.name.trim().to_owned();
        self.host = self.host.trim().to_lowercase();
        self.username = self.username.trim().to_owned();
        if self.host.starts_with('[') && self.host.ends_with(']') {
            self.host = self.host[1..self.host.len() - 1].to_owned();
        }
        if self.host.contains(':') {
            self.host = self
                .host
                .parse::<std::net::IpAddr>()
                .map_err(|_| {
                    AppError::new(
                        "INVALID_PROFILE",
                        "Informe apenas o host/IP. Use o campo Porta para a porta SSH.",
                    )
                })?
                .to_string();
        }
        if self.name.is_empty()
            || self.name.len() > 120
            || self.name.chars().any(char::is_control)
            || self.port == 0
            || self.host.is_empty()
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "/@\\".contains(c))
            || self.username.is_empty()
            || self.username.len() > 128
            || self
                .username
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(AppError::new(
                "INVALID_PROFILE",
                "Confira nome, host, porta (1–65535) e usuário.",
            ));
        }
        if let Some(path) = &self.initial_directory {
            if path.is_empty() {
                self.initial_directory = None;
            } else if path.len() > 4096
                || !path.starts_with('/')
                || path.chars().any(char::is_control)
            {
                return Err(AppError::new(
                    "INVALID_PROFILE",
                    "Use um diretório inicial absoluto, sem caracteres de controle.",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredProfile {
    #[serde(flatten)]
    pub fields: ProfileInput,
    pub secret_id: Option<Uuid>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(flatten)]
    pub fields: ProfileInput,
    pub has_saved_secret: bool,
}
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Secret {
    Password {
        password: String,
    },
    Key {
        private_key: String,
        passphrase: String,
    },
}
impl Secret {
    pub fn validate(&self, mode: AuthMode) -> Result<()> {
        let valid = match self {
            Self::Password { password } => {
                mode == AuthMode::Password && !password.is_empty() && password.len() <= 4096
            }
            Self::Key {
                private_key,
                passphrase,
            } => {
                mode == AuthMode::Key
                    && private_key.len() <= 1024 * 1024
                    && passphrase.len() <= 4096
                    && private_key.contains("PRIVATE KEY")
            }
        };
        if !valid {
            return Err(AppError::new(
                "INVALID_CREDENTIAL",
                "Confira a credencial e o tipo de autenticação.",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct StoredSecret {
    id: Uuid,
    material: Secret,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretPolicy {
    Keep,
    Save,
    Forget,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub key: String,
    pub fingerprint: String,
}
fn unknown() -> AppError {
    AppError::new("UNKNOWN_PROFILE", "Conexão desconhecida ou removida.")
}
fn stored(payload: &crate::services::vault::VaultPayload) -> Result<Vec<StoredProfile>> {
    payload
        .profiles
        .iter()
        .map(|v| serde_json::from_value(v.clone()).map_err(|_| AppError::invalid()))
        .collect()
}
fn dto(p: &StoredProfile) -> Profile {
    Profile {
        fields: p.fields.clone(),
        has_saved_secret: p.secret_id.is_some(),
    }
}
impl Vault {
    pub fn profiles(&self, generation: u64) -> Result<Vec<Profile>> {
        self.authorize(generation)?;
        Ok(stored(&self.payload()?)?.iter().map(dto).collect())
    }
    pub fn profile(&self, generation: u64, id: Uuid) -> Result<StoredProfile> {
        self.authorize(generation)?;
        stored(&self.payload()?)?
            .into_iter()
            .find(|p| p.fields.id == Some(id))
            .ok_or_else(unknown)
    }
    pub fn put_profile(
        &mut self,
        generation: u64,
        mut fields: ProfileInput,
        policy: SecretPolicy,
        secret: Option<Secret>,
    ) -> Result<Profile> {
        self.authorize(generation)?;
        fields.validate()?;
        if matches!(policy, SecretPolicy::Save) {
            secret
                .as_ref()
                .ok_or_else(AppError::invalid)?
                .validate(fields.auth_mode)?;
        }
        self.transaction(generation, move |payload| {
            let mut profiles = stored(payload)?;
            if profiles.len() >= 1000 && fields.id.is_none() {
                return Err(AppError::new(
                    "LIMIT_REACHED",
                    "Limite de conexões atingido.",
                ));
            }
            let previous = if let Some(id) = fields.id {
                Some(
                    profiles
                        .iter()
                        .find(|p| p.fields.id == Some(id))
                        .cloned()
                        .ok_or_else(unknown)?,
                )
            } else {
                None
            };
            fields.id = Some(fields.id.unwrap_or_else(Uuid::new_v4));
            let old = previous.as_ref().and_then(|p| p.secret_id);
            let keep = matches!(policy, SecretPolicy::Keep)
                && previous
                    .as_ref()
                    .is_some_and(|p| p.fields.auth_mode == fields.auth_mode);
            if !keep {
                payload.secrets.retain(|v| {
                    v.get("id")
                        .and_then(|v| v.as_str())
                        .and_then(|v| Uuid::parse_str(v).ok())
                        != old
                });
            }
            let secret_id = if matches!(policy, SecretPolicy::Save) {
                let id = Uuid::new_v4();
                payload.secrets.push(
                    serde_json::to_value(StoredSecret {
                        id,
                        material: secret.ok_or_else(AppError::invalid)?,
                    })
                    .map_err(|_| AppError::invalid())?,
                );
                Some(id)
            } else if keep {
                old
            } else {
                None
            };
            let result = StoredProfile { fields, secret_id };
            profiles.retain(|p| p.fields.id != result.fields.id);
            profiles.push(result.clone());
            payload.profiles = profiles
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| AppError::invalid())?;
            Ok(dto(&result))
        })
    }
    pub fn duplicate_profile(&mut self, generation: u64, id: Uuid) -> Result<Profile> {
        let mut profile = self.profile(generation, id)?.fields;
        profile.id = None;
        while profile.name.len() > 110 {
            profile.name.pop();
        }
        profile.name = format!("{} (cópia)", profile.name);
        // Always independent: duplication never copies credentials.
        self.put_profile(generation, profile, SecretPolicy::Forget, None)
    }
    pub fn delete_profile(&mut self, generation: u64, id: Uuid) -> Result<()> {
        let profile = self.profile(generation, id)?;
        self.transaction(generation, |payload| {
            payload
                .profiles
                .retain(|v| v.get("id").and_then(|v| v.as_str()) != Some(&id.to_string()));
            payload.compose_projects.retain(|v| {
                v.pointer("/identity/profileId").and_then(|v| v.as_str()) != Some(&id.to_string())
            });
            if let Some(secret) = profile.secret_id {
                payload
                    .secrets
                    .retain(|v| v.get("id").and_then(|v| v.as_str()) != Some(&secret.to_string()));
            }
            Ok(())
        })
    }
    pub fn credential(&self, generation: u64, profile: &StoredProfile) -> Result<Option<Secret>> {
        self.authorize(generation)?;
        let Some(id) = profile.secret_id else {
            return Ok(None);
        };
        let payload = self.payload()?;
        for value in &payload.secrets {
            let entry: StoredSecret =
                serde_json::from_value(value.clone()).map_err(|_| AppError::invalid())?;
            if entry.id == id {
                return Ok(Some(entry.material.clone()));
            }
        }
        Err(AppError::new(
            "INVALID_VAULT",
            "Referência de credencial ausente no cofre.",
        ))
    }
    pub fn known_host(&self, generation: u64, host: &str, port: u16) -> Result<Option<KnownHost>> {
        self.authorize(generation)?;
        for value in &self.payload()?.known_hosts {
            let known: KnownHost =
                serde_json::from_value(value.clone()).map_err(|_| AppError::invalid())?;
            if known.host == host && known.port == port {
                return Ok(Some(known));
            }
        }
        Ok(None)
    }
    pub fn trust_host_if_unchanged(
        &mut self,
        generation: u64,
        expected: Option<String>,
        known: KnownHost,
    ) -> Result<()> {
        let current = self
            .known_host(generation, &known.host, known.port)?
            .map(|host| host.key);
        let unchanged = match (&current, &expected) {
            (None, None) => true,
            (Some(a), Some(b)) => crate::adapters::ssh::same_public_key(a, b),
            _ => false,
        };
        let already_trusted = current
            .as_ref()
            .is_some_and(|key| crate::adapters::ssh::same_public_key(key, &known.key));
        if !unchanged && !already_trusted {
            return Err(AppError::new(
                "HOST_KEY_CHANGED",
                "A identidade salva mudou durante a confirmação. Conecte novamente para revisar.",
            ));
        }
        self.trust_host(generation, known)
    }
    pub fn trust_host(&mut self, generation: u64, known: KnownHost) -> Result<()> {
        self.transaction(generation, |payload| {
            payload.known_hosts.retain(|v| {
                v.get("host").and_then(|v| v.as_str()) != Some(&known.host)
                    || v.get("port").and_then(|v| v.as_u64()) != Some(known.port.into())
            });
            payload
                .known_hosts
                .push(serde_json::to_value(known).map_err(|_| AppError::invalid())?);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let mut vault =
            Vault::new(crate::adapters::storage::Storage::open(dir.path().join("vault")).unwrap());
        vault.create("disposable master fixture").unwrap();
        (dir, vault)
    }
    pub fn input() -> ProfileInput {
        ProfileInput {
            id: None,
            name: "Fixture".into(),
            host: "example.invalid".into(),
            port: 22,
            username: "fixture".into(),
            auth_mode: AuthMode::Password,
            initial_directory: None,
        }
    }
    #[test]
    fn crud_keeps_secrets_private_and_duplicate_independent() {
        let (dir, mut vault) = fixture();
        let saved = vault
            .put_profile(
                1,
                input(),
                SecretPolicy::Save,
                Some(Secret::Password {
                    password: "synthetic credential".into(),
                }),
            )
            .unwrap();
        let listed = serde_json::to_string(&vault.profiles(1).unwrap()).unwrap();
        assert!(!listed.contains("synthetic credential"));
        assert!(!listed.contains("secretId"));
        assert!(saved.has_saved_secret);
        let copied = vault
            .duplicate_profile(1, saved.fields.id.unwrap())
            .unwrap();
        assert!(!copied.has_saved_secret);
        let bytes = std::fs::read(dir.path().join("vault/vault.json")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("example.invalid"));
        vault.lock();
        vault.unlock("disposable master fixture").unwrap();
        assert_eq!(vault.profiles(2).unwrap().len(), 2);
        let profile = vault.profile(2, saved.fields.id.unwrap()).unwrap();
        assert!(vault.credential(2, &profile).unwrap().is_some());
        let mut changed = profile.fields.clone();
        changed.auth_mode = AuthMode::Agent;
        vault
            .put_profile(2, changed, SecretPolicy::Keep, None)
            .unwrap();
        assert!(vault.payload().unwrap().secrets.is_empty());
        vault.delete_profile(2, copied.fields.id.unwrap()).unwrap();
        assert_eq!(vault.profiles(2).unwrap().len(), 1);
        assert!(vault.profiles(1).is_err());
    }
    #[test]
    fn transient_inputs_are_not_persisted_and_locked_access_denied() {
        let (_dir, mut vault) = fixture();
        let profile = vault
            .put_profile(
                1,
                input(),
                SecretPolicy::Forget,
                Some(Secret::Password {
                    password: "never saved".into(),
                }),
            )
            .unwrap();
        assert!(!profile.has_saved_secret);
        assert!(vault.payload().unwrap().secrets.is_empty());
        vault.lock();
        assert_eq!(vault.profiles(2).unwrap_err().code, "VAULT_LOCKED");
    }
    #[test]
    fn invalid_fields_rejected() {
        for mutate in [
            |p: &mut ProfileInput| p.port = 0,
            |p: &mut ProfileInput| p.host = "bad host".into(),
            |p: &mut ProfileInput| p.username = "".into(),
            |p: &mut ProfileInput| p.initial_directory = Some("relative".into()),
        ] {
            let mut p = input();
            mutate(&mut p);
            assert!(p.validate().is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn failed_write_preserves_memory_and_disk() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut vault) = fixture();
        let path = dir.path().join("vault");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = vault.put_profile(1, input(), SecretPolicy::Forget, None);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(vault.profiles(1).unwrap().is_empty());
        vault.lock();
        vault.unlock("disposable master fixture").unwrap();
        assert!(vault.profiles(2).unwrap().is_empty());
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn concurrent_reviews_cannot_silently_replace_a_newly_trusted_host() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault =
            Vault::new(crate::adapters::storage::Storage::open(dir.path().join("vault")).unwrap());
        vault.create("disposable master fixture").unwrap();
        let first =
            russh::keys::PrivateKey::random(&mut rand10::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let second =
            russh::keys::PrivateKey::random(&mut rand10::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let a = KnownHost {
            host: "example.invalid".into(),
            port: 22,
            key: first.public_key().to_openssh().unwrap(),
            fingerprint: "a".into(),
        };
        let b = KnownHost {
            key: second.public_key().to_openssh().unwrap(),
            fingerprint: "b".into(),
            ..a.clone()
        };
        vault.trust_host_if_unchanged(1, None, a.clone()).unwrap();
        assert_eq!(
            vault
                .trust_host_if_unchanged(1, None, b.clone())
                .unwrap_err()
                .code,
            "HOST_KEY_CHANGED"
        );
        assert_eq!(
            vault
                .known_host(1, "example.invalid", 22)
                .unwrap()
                .unwrap()
                .key,
            a.key
        );
        vault
            .trust_host_if_unchanged(1, Some(a.key), b.clone())
            .unwrap();
        assert_eq!(
            vault
                .known_host(1, "example.invalid", 22)
                .unwrap()
                .unwrap()
                .key,
            b.key
        );
    }
}
