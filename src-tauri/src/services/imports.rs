use crate::{
    ipc::{AppError, Result},
    services::profiles::{AuthMode, Secret},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialInput {
    pub password: Option<String>,
    pub key_import: Option<Uuid>,
    pub passphrase: Option<String>,
}
impl Drop for CredentialInput {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(value) = &mut self.password {
            value.zeroize();
        }
        if let Some(value) = &mut self.passphrase {
            value.zeroize();
        }
    }
}
struct ImportedKey {
    generation: u64,
    created: Instant,
    contents: Zeroizing<String>,
}
#[derive(Default)]
pub struct Imports {
    entries: HashMap<Uuid, ImportedKey>,
}
impl Imports {
    pub fn insert(&mut self, generation: u64, contents: Zeroizing<String>) -> Result<Uuid> {
        self.entries.retain(|_, e| {
            e.created.elapsed() < Duration::from_secs(300) && e.generation == generation
        });
        if self.entries.len() >= 8 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Limite de importações temporárias atingido. Aguarde cinco minutos.",
            ));
        }
        let id = Uuid::new_v4();
        self.entries.insert(
            id,
            ImportedKey {
                generation,
                created: Instant::now(),
                contents,
            },
        );
        Ok(id)
    }
    pub fn resolve(
        &mut self,
        generation: u64,
        mode: AuthMode,
        input: CredentialInput,
    ) -> Result<Secret> {
        let secret = match mode {
            AuthMode::Password => Secret::Password {
                password: input.password.clone().ok_or_else(AppError::invalid)?,
            },
            AuthMode::Key => {
                let id = input.key_import.ok_or_else(AppError::invalid)?;
                let imported = self.entries.remove(&id).ok_or_else(|| {
                    AppError::new(
                        "IMPORT_EXPIRED",
                        "Importação expirada. Selecione a chave novamente.",
                    )
                })?;
                if imported.generation != generation
                    || imported.created.elapsed() >= Duration::from_secs(300)
                {
                    return Err(AppError::new(
                        "IMPORT_EXPIRED",
                        "Importação expirada. Selecione a chave novamente.",
                    ));
                }
                Secret::Key {
                    private_key: imported.contents.to_string(),
                    passphrase: input.passphrase.clone().unwrap_or_default(),
                }
            }
            AuthMode::Agent => return Err(AppError::invalid()),
        };
        secret.validate(mode)?;
        Ok(secret)
    }
    pub fn cleanup(&mut self) {
        self.entries
            .retain(|_, entry| entry.created.elapsed() < Duration::from_secs(300));
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
