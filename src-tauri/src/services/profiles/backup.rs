use super::{AuthMode, ProfileInput, StoredProfile, stored};
use crate::{
    ipc::{AppError, Result},
    services::vault::Vault,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const NOTE: &str = "Senhas, passphrases e chaves privadas não são incluídas por segurança.";
fn default_icon() -> String {
    "ssh".into()
}
fn invalid() -> AppError {
    AppError::new(
        "INVALID_BACKUP",
        "JSON de conexões inválido ou incompatível. Nenhuma conexão foi importada.",
    )
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn timestamp(value: u64) -> bool {
    value <= 9_007_199_254_740_991
}
fn text(value: &str, max: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.len() <= max
        && !value.chars().any(char::is_control)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: Uuid,
    pub name: String,
    pub created_at: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub icon: String,
    pub key_file_name: String,
    pub folder_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
}
impl Default for Metadata {
    fn default() -> Self {
        let time = now();
        Self {
            icon: "ssh".into(),
            key_file_name: String::new(),
            folder_id: None,
            created_at: time,
            updated_at: time,
        }
    }
}
// Separate whitelist: neither StoredProfile nor Secret is serialized into a backup.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_icon", skip_serializing)]
    pub icon: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthMode,
    pub key_file_name: String,
    #[serde(default, skip_serializing)]
    pub folder_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_directory: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing)]
    pub folders: Vec<Folder>,
    pub connections: Vec<Connection>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub folders_imported: usize,
}
impl Backup {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid());
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let mut backup: Self = serde_json::from_slice(bytes).map_err(|_| invalid())?;
        backup.validate()?;
        Ok(backup)
    }
    fn validate(&mut self) -> Result<()> {
        if self.format != "ssh-client-backup"
            || self.version != 1
            || self.folders.len() > 1000
            || self.connections.len() > 1000
            || chrono::DateTime::parse_from_rfc3339(&self.exported_at).is_err()
        {
            return Err(invalid());
        }
        let mut folders = HashSet::new();
        for folder in &self.folders {
            if !folders.insert(folder.id)
                || !text(&folder.name, 120, false)
                || !timestamp(folder.created_at)
            {
                return Err(invalid());
            }
        }
        let mut ids = HashSet::new();
        for connection in &mut self.connections {
            let mut fields = connection.fields();
            fields.validate().map_err(|_| invalid())?;
            if !ids.insert(connection.id)
                || !text(&connection.icon, 64, false)
                || !connection
                    .icon
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                || !text(&connection.key_file_name, 255, true)
                || connection.key_file_name.contains(['/', '\\'])
                || !timestamp(connection.created_at)
                || !timestamp(connection.updated_at)
                || connection
                    .folder_id
                    .is_some_and(|id| !folders.contains(&id))
            {
                return Err(invalid());
            }
            connection.name = fields.name;
            connection.host = fields.host;
            connection.username = fields.username;
            connection.initial_directory = fields.initial_directory;
        }
        Ok(())
    }
}
impl Connection {
    fn fields(&self) -> ProfileInput {
        ProfileInput {
            id: Some(self.id),
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            auth_mode: self.auth_type,
            initial_directory: self.initial_directory.clone(),
        }
    }
    fn metadata(&self) -> Metadata {
        Metadata {
            icon: self.icon.clone(),
            key_file_name: self.key_file_name.clone(),
            folder_id: self.folder_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
impl Vault {
    pub fn export_profiles(&self, generation: u64) -> Result<Vec<u8>> {
        self.authorize(generation)?;
        let payload = self.payload()?;
        let profiles = stored(&payload)?;
        let folders = payload
            .profile_folders
            .iter()
            .map(|v| serde_json::from_value(v.clone()).map_err(|_| invalid()))
            .collect::<Result<Vec<Folder>>>()?;
        let mut connections = Vec::new();
        for profile in profiles {
            let meta = profile.backup.unwrap_or_default();
            let fields = profile.fields;
            connections.push(Connection {
                id: fields.id.ok_or_else(invalid)?,
                name: fields.name,
                icon: meta.icon,
                host: fields.host,
                port: fields.port,
                username: fields.username,
                auth_type: fields.auth_mode,
                key_file_name: meta.key_file_name,
                folder_id: meta.folder_id,
                created_at: meta.created_at,
                updated_at: meta.updated_at,
                initial_directory: fields.initial_directory,
            });
        }
        let exported_at: chrono::DateTime<chrono::Utc> = SystemTime::now().into();
        let backup = Backup {
            format: "ssh-client-backup".into(),
            version: 1,
            exported_at: exported_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            note: NOTE.into(),
            folders,
            connections,
        };
        let result = serde_json::to_vec_pretty(&backup).map_err(|_| invalid())?;
        if result.len() > MAX_BYTES {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "O JSON de conexões excede 2 MiB.",
            ));
        }
        Ok(result)
    }
    pub fn import_profiles(&mut self, generation: u64, mut backup: Backup) -> Result<ImportResult> {
        self.authorize(generation)?;
        backup.validate()?;
        self.transaction(generation, move |payload| {
            let mut profiles = stored(payload)?;
            let mut ids = profiles
                .iter()
                .filter_map(|p| p.fields.id)
                .collect::<HashSet<_>>();
            let mut folders = payload
                .profile_folders
                .iter()
                .map(|v| serde_json::from_value(v.clone()).map_err(|_| invalid()))
                .collect::<Result<Vec<Folder>>>()?;
            let mut folder_ids = folders.iter().map(|f| f.id).collect::<HashSet<_>>();
            let mut result = ImportResult {
                imported: 0,
                skipped: 0,
                folders_imported: 0,
            };
            for folder in backup.folders {
                if folder_ids.insert(folder.id) {
                    folders.push(folder);
                    result.folders_imported += 1;
                }
            }
            for connection in backup.connections {
                if !ids.insert(connection.id) {
                    result.skipped += 1;
                    continue;
                }
                profiles.push(StoredProfile {
                    fields: connection.fields(),
                    secret_id: None,
                    backup: Some(connection.metadata()),
                });
                result.imported += 1;
            }
            if profiles.len() > 1000 || folders.len() > 1000 {
                return Err(AppError::new(
                    "LIMIT_REACHED",
                    "Limite de 1000 conexões ou pastas atingido. Nada foi importado.",
                ));
            }
            payload.profiles = profiles
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| invalid())?;
            payload.profile_folders = folders
                .iter()
                .map(serde_json::to_value)
                .collect::<std::result::Result<_, _>>()
                .map_err(|_| invalid())?;
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::storage::Storage;
    use crate::services::profiles::{Secret, SecretPolicy};
    use serde_json::{Value, json};
    fn fixture() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
        vault.create("disposable backup fixture").unwrap();
        (dir, vault)
    }
    fn document() -> Value {
        json!({"format":"ssh-client-backup","version":1,"exportedAt":"2026-11-06T13:23:57.763Z","note":"original",
            "folders":[{"id":"00000000-0000-4000-8000-000000000001","name":"Homologação","createdAt":1785262762003u64}],
            "connections":[{"id":"00000000-0000-4000-8000-000000000002","name":"Servidor","icon":"twingate","host":"example.invalid","port":2222,"username":"tester","authType":"key","keyFileName":"test.ppk","folderId":"00000000-0000-4000-8000-000000000001","createdAt":1785261509468u64,"updatedAt":1785262788054u64,"initialDirectory":"/srv/app"}]})
    }
    fn parse(value: &Value) -> Backup {
        Backup::parse(&serde_json::to_vec(value).unwrap()).unwrap()
    }
    #[test]
    fn roundtrip_preserves_metadata_after_restart_and_import_is_idempotent() {
        let (dir, mut vault) = fixture();
        let original = document();
        let result = vault.import_profiles(1, parse(&original)).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.folders_imported, 1);
        assert!(!vault.profiles(1).unwrap()[0].has_saved_secret);
        let second = vault.import_profiles(1, parse(&original)).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped, 1);
        drop(vault);
        let mut reopened = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
        reopened.unlock("disposable backup fixture").unwrap();
        let value: Value = serde_json::from_slice(
            &reopened
                .export_profiles(reopened.status().generation)
                .unwrap(),
        )
        .unwrap();
        assert!(value.get("folders").is_none());
        let mut expected = original["connections"].clone();
        for connection in expected.as_array_mut().unwrap() {
            connection.as_object_mut().unwrap().remove("icon");
            connection.as_object_mut().unwrap().remove("folderId");
        }
        assert_eq!(value["connections"], expected);
        let stored = stored(&reopened.payload().unwrap()).unwrap();
        let metadata = stored[0].backup.as_ref().unwrap();
        assert_eq!(metadata.icon, "twingate");
        assert!(metadata.folder_id.is_some());
        assert_eq!(reopened.payload().unwrap().profile_folders.len(), 1);
        assert_eq!(value["format"], "ssh-client-backup");
        assert_eq!(value["version"], 1);
        assert!(
            chrono::DateTime::parse_from_rfc3339(value["exportedAt"].as_str().unwrap()).is_ok()
        );
        assert!(
            !String::from_utf8_lossy(&std::fs::read(dir.path().join("vault/vault.json")).unwrap())
                .contains("example.invalid")
        );
    }
    #[test]
    fn whitelisted_export_and_additive_import_never_touch_credentials() {
        let (_dir, mut vault) = fixture();
        let mut input = parse(&document()).connections.remove(0).fields();
        input.id = None;
        input.auth_mode = AuthMode::Password;
        input.name = "Original".into();
        let saved = vault
            .put_profile(
                1,
                input,
                SecretPolicy::Save,
                Some(Secret::Password {
                    password: "saved-synthetic-password".into(),
                }),
            )
            .unwrap();
        let mut source = document();
        source["password"] = "root-secret".into();
        source["note"] = "secret-in-note".into();
        source["connections"][0]["password"] = "injected-synthetic-password".into();
        source["connections"][0]["privateKey"] = "injected-private-key".into();
        source["connections"][0]["passphrase"] = "injected-passphrase".into();
        let mut collision = source["connections"][0].clone();
        collision["id"] = json!(saved.fields.id.unwrap());
        collision["host"] = json!("changed.invalid");
        source["connections"]
            .as_array_mut()
            .unwrap()
            .push(collision);
        let result = vault.import_profiles(1, parse(&source)).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.skipped, 1);
        let stored = vault.profile(1, saved.fields.id.unwrap()).unwrap();
        assert_eq!(stored.fields.name, "Original");
        assert_eq!(stored.fields.host, "example.invalid");
        assert!(
            matches!(vault.credential(1,&stored).unwrap(),Some(Secret::Password {ref password}) if password=="saved-synthetic-password")
        );
        let encoded = String::from_utf8(vault.export_profiles(1).unwrap()).unwrap();
        for secret in [
            "root-secret",
            "secret-in-note",
            "saved-synthetic-password",
            "injected-synthetic-password",
            "injected-private-key",
            "injected-passphrase",
            "secretId",
            "hasSavedSecret",
            "privateKey",
        ] {
            assert!(
                !encoded.contains(secret),
                "Unexpected export field/material"
            );
        }
        let value: Value = serde_json::from_str(&encoded).unwrap();
        for connection in value["connections"].as_array().unwrap() {
            for field in [
                "password",
                "passphrase",
                "privateKey",
                "secretId",
                "hasSavedSecret",
            ] {
                assert!(connection.get(field).is_none());
            }
        }
        let imported = vault
            .profile(
                1,
                Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
            )
            .unwrap();
        assert!(vault.credential(1, &imported).unwrap().is_none());
    }
    #[test]
    fn rejects_invalid_formats_entries_links_duplicates_and_oversized_input() {
        let source = document();
        for (pointer, value) in [
            ("/format", json!("other")),
            ("/version", json!(2)),
            ("/exportedAt", json!("bad date")),
            ("/connections/0/port", json!(0)),
            ("/connections/0/host", json!("bad host")),
            ("/connections/0/authType", json!("unknown")),
            (
                "/connections/0/folderId",
                json!("00000000-0000-4000-8000-000000000099"),
            ),
            ("/connections/0/keyFileName", json!("/private/key")),
            ("/connections/0/updatedAt", json!(-1)),
        ] {
            let mut candidate = source.clone();
            *candidate.pointer_mut(pointer).unwrap() = value;
            assert!(Backup::parse(&serde_json::to_vec(&candidate).unwrap()).is_err());
        }
        assert!(Backup::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(Backup::parse(b"not JSON").is_err());
        let mut duplicate = source.clone();
        duplicate["connections"]
            .as_array_mut()
            .unwrap()
            .push(source["connections"][0].clone());
        assert!(Backup::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
        let mut bom = b"\xef\xbb\xbf".to_vec();
        bom.extend(serde_json::to_vec(&source).unwrap());
        assert!(Backup::parse(&bom).is_ok());
    }
    #[test]
    fn supplied_reference_imports_and_exports_in_the_same_schema() {
        let source = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../docs/ssh-sessions-2026-11-15.json"),
        )
        .unwrap();
        let expected: Value = serde_json::from_slice(&source).unwrap();
        let (_dir, mut vault) = fixture();
        let result = vault
            .import_profiles(1, Backup::parse(&source).unwrap())
            .unwrap();
        assert_eq!(
            result.imported,
            expected["connections"].as_array().unwrap().len()
        );
        let exported: Value = serde_json::from_slice(&vault.export_profiles(1).unwrap()).unwrap();
        assert!(exported.get("folders").is_none());
        assert_eq!(exported["connections"], expected["connections"]);
    }
    #[test]
    fn minimal_schema_defaults_legacy_metadata_and_exports_only_current_fields() {
        let mut source = document();
        source.as_object_mut().unwrap().remove("folders");
        let connection = source["connections"][0].as_object_mut().unwrap();
        connection.remove("icon");
        connection.remove("folderId");
        let (_dir, mut vault) = fixture();
        let result = vault.import_profiles(1, parse(&source)).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.folders_imported, 0);
        let exported: Value = serde_json::from_slice(&vault.export_profiles(1).unwrap()).unwrap();
        assert_eq!(exported["connections"], source["connections"]);
        assert!(exported.get("folders").is_none());
        let reparsed = Backup::parse(&serde_json::to_vec(&exported).unwrap()).unwrap();
        assert_eq!(reparsed.connections[0].icon, "ssh");
        assert!(reparsed.connections[0].folder_id.is_none());
        assert_eq!(vault.import_profiles(1, reparsed).unwrap().skipped, 1);
    }
    #[cfg(unix)]
    #[test]
    fn failed_publication_keeps_profiles_folders_and_file_unchanged() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut vault) = fixture();
        let path = dir.path().join("vault");
        let original = std::fs::read(path.join("vault.json")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = vault.import_profiles(1, parse(&document()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert!(vault.profiles(1).unwrap().is_empty());
        assert!(vault.payload().unwrap().profile_folders.is_empty());
        assert_eq!(std::fs::read(path.join("vault.json")).unwrap(), original);
    }
    #[test]
    fn reads_payloads_and_profiles_created_before_backup_support() {
        let value = json!({"profiles":[{"id":"00000000-0000-4000-8000-000000000003","name":"Legacy","host":"legacy.invalid","port":22,"username":"user","authMode":"agent","initialDirectory":null,"secretId":null}],"secrets":[],"known_hosts":[],"draft_index":[]});
        let payload: crate::services::vault::VaultPayload = serde_json::from_value(value).unwrap();
        assert!(payload.profile_folders.is_empty());
        let profiles = stored(&payload).unwrap();
        assert!(profiles[0].backup.is_none());
    }
    #[test]
    fn invalid_batch_is_atomic_and_generation_is_required() {
        let (dir, mut vault) = fixture();
        let original = std::fs::read(dir.path().join("vault/vault.json")).unwrap();
        let mut batch = parse(&document());
        let mut bad = parse(&document()).connections.remove(0);
        bad.id = Uuid::new_v4();
        bad.port = 0;
        batch.connections.push(bad);
        assert!(vault.import_profiles(1, batch).is_err());
        assert!(vault.profiles(1).unwrap().is_empty());
        assert_eq!(
            original,
            std::fs::read(dir.path().join("vault/vault.json")).unwrap()
        );
        vault.lock();
        assert!(vault.import_profiles(1, parse(&document())).is_err());
        assert!(vault.export_profiles(1).is_err());
        let generation = vault
            .unlock("disposable backup fixture")
            .unwrap()
            .generation;
        assert!(vault.export_profiles(generation - 1).is_err());
        assert!(vault.export_profiles(generation).is_ok());
    }
}
