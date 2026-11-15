use super::*;
use crate::services::{files::documents::Revision, profiles::ProfileInput};
use uuid::Uuid;
use zeroize::Zeroize;
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
    pub profile_id: Uuid,
    pub host: String,
    pub port: u16,
    pub username: String,
}
impl Identity {
    pub fn from_profile(profile: &ProfileInput) -> Result<Self> {
        Ok(Self {
            profile_id: profile.id.ok_or_else(AppError::invalid)?,
            host: profile.host.clone(),
            port: profile.port,
            username: profile.username.clone(),
        })
    }
    pub fn matches(&self, profile: &ProfileInput) -> bool {
        Self::from_profile(profile).is_ok_and(|other| other == *self)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub id: Uuid,
    pub identity: Identity,
    pub name: String,
    pub path: Vec<u8>,
    pub text: String,
    pub revision: Revision,
    pub bom: bool,
    pub endings: Vec<u8>,
    pub preferred: u8,
    pub permissions: Option<u32>,
}
impl Drop for Draft {
    fn drop(&mut self) {
        self.text.zeroize();
        self.path.zeroize();
        self.name.zeroize();
        self.identity.host.zeroize();
        self.identity.username.zeroize();
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftInfo {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub server: String,
    pub name: String,
    pub path: String,
}
fn aad(reference: &DraftRef) -> Vec<u8> {
    let mut bytes = b"unny-draft-v1".to_vec();
    bytes.extend(reference.id.as_bytes());
    bytes.extend(reference.blob.as_bytes());
    bytes.extend(reference.generation.to_be_bytes());
    bytes
}
pub(super) fn write(
    storage: &Storage,
    key: &[u8; 32],
    generation: u64,
    draft: &Draft,
) -> Result<DraftRef> {
    let reference = DraftRef {
        id: draft.id,
        generation,
        blob: Uuid::new_v4(),
    };
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let plaintext = Zeroizing::new(serde_json::to_vec(draft).map_err(|_| invalid())?);
    let ciphertext = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| invalid())?
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &aad(&reference),
            },
        )
        .map_err(|_| invalid())?;
    let mut bytes = nonce.to_vec();
    bytes.extend(ciphertext);
    storage.write_blob(reference.blob, &bytes)?;
    Ok(reference)
}
pub(super) fn read(storage: &Storage, key: &[u8; 32], reference: &DraftRef) -> Result<Draft> {
    let bytes = storage.read_blob(reference.blob)?;
    if bytes.len() < 40 {
        return Err(invalid());
    }
    let plain = Zeroizing::new(
        XChaCha20Poly1305::new_from_slice(key)
            .map_err(|_| invalid())?
            .decrypt(
                XNonce::from_slice(&bytes[..24]),
                Payload {
                    msg: &bytes[24..],
                    aad: &aad(reference),
                },
            )
            .map_err(|_| invalid())?,
    );
    let draft: Draft = serde_json::from_slice(&plain).map_err(|_| invalid())?;
    if draft.id != reference.id {
        return Err(invalid());
    }
    Ok(draft)
}
impl Vault {
    pub fn lock_drafts(&mut self, generation: u64, drafts: Vec<Draft>) -> Result<VaultStatus> {
        self.authorize(generation)?;
        if drafts.is_empty() {
            return Ok(self.lock());
        }
        self.generation += 1;
        self.pending = Some(drafts);
        if self.persist_pending().is_ok() {
            self.pending = None;
            self.open = None;
        }
        Ok(self.status())
    }
    fn persist_pending(&mut self) -> Result<()> {
        let drafts = self.pending.as_ref().ok_or_else(AppError::invalid)?;
        if drafts.len() > 32
            || drafts.iter().map(|d| d.text.len()).sum::<usize>() > 128 * 1024 * 1024
        {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Rascunhos excedem 128 MiB ou 32 documentos.",
            ));
        }
        let current = self.open.as_ref().ok_or_else(invalid)?;
        let mut payload = self.payload()?;
        let mut obsolete = Vec::new();
        for draft in drafts {
            let reference = write(
                &self.storage,
                &current.key,
                current
                    .header
                    .generation
                    .checked_add(1)
                    .ok_or_else(invalid)?,
                draft,
            )?;
            if let Some(index) = payload.draft_index.iter().position(|r| r.id == draft.id) {
                obsolete.push(payload.draft_index.remove(index).blob);
            }
            payload.draft_index.push(reference);
        }
        if payload.draft_index.len() > 64 {
            return Err(AppError::new(
                "LIMIT_REACHED",
                "Remova rascunhos antigos antes de proteger mais documentos (limite 64).",
            ));
        }
        self.publish_payload(&payload)?;
        for id in obsolete {
            self.storage.remove_blob(id);
        }
        Ok(())
    }
    pub fn recover_lock(&mut self, password: &str, discard: bool) -> Result<VaultStatus> {
        if self.pending.is_none() {
            return Err(AppError::invalid());
        }
        let current = self.open.as_ref().ok_or_else(invalid)?;
        // Verify authentication by decrypting an authenticated envelope; no password is retained.
        let bytes = encode(current)?;
        let _ = decode(&bytes, password)?;
        if discard {
            self.pending = None;
            self.open = None;
        } else {
            self.persist_pending()?;
            self.pending = None;
        }
        Ok(self.status())
    }
    pub fn draft(&self, generation: u64, id: Uuid) -> Result<Draft> {
        self.authorize(generation)?;
        let payload = self.payload()?;
        let reference = payload
            .draft_index
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(AppError::invalid)?;
        read(
            &self.storage,
            &self.open.as_ref().ok_or_else(invalid)?.key,
            reference,
        )
    }
    pub fn drafts(&self, generation: u64) -> Result<Vec<DraftInfo>> {
        self.authorize(generation)?;
        let payload = self.payload()?;
        let key = &self.open.as_ref().ok_or_else(invalid)?.key;
        payload
            .draft_index
            .iter()
            .map(|r| {
                let d = read(&self.storage, key, r)?;
                Ok(DraftInfo {
                    id: d.id,
                    profile_id: d.identity.profile_id,
                    server: format!(
                        "{}@{}:{}",
                        d.identity.username, d.identity.host, d.identity.port
                    ),
                    name: d.name.clone(),
                    path: crate::services::files::display(&d.path),
                })
            })
            .collect()
    }
    pub fn remove_draft(&mut self, generation: u64, id: Uuid) -> Result<()> {
        let blob = self.transaction(generation, |p| {
            let index = p
                .draft_index
                .iter()
                .position(|r| r.id == id)
                .ok_or_else(AppError::invalid)?;
            Ok(p.draft_index.remove(index).blob)
        })?;
        self.storage.remove_blob(blob);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Vault) {
        let d = tempfile::tempdir().unwrap();
        let mut v = Vault::new(Storage::open(d.path().join("vault")).unwrap());
        v.create("disposable master fixture").unwrap();
        (d, v)
    }
    fn draft() -> Draft {
        Draft {
            id: Uuid::new_v4(),
            identity: Identity {
                profile_id: Uuid::new_v4(),
                host: "fixture.invalid".into(),
                port: 22,
                username: "fixture".into(),
            },
            name: "note.txt".into(),
            path: b"/note_\xff.txt".to_vec(),
            text: "unsaved fixture text\n".into(),
            revision: Revision {
                size: 4,
                mtime: Some(10),
                sha256: "fixture revision".into(),
            },
            bom: true,
            endings: vec![2],
            preferred: 2,
            permissions: Some(0o640),
        }
    }
    #[test]
    fn blobs_survive_restart_and_password_rotation() {
        let (dir, mut v) = setup();
        let draft = draft();
        let id = draft.id;
        assert_eq!(v.lock_drafts(1, vec![draft]).unwrap().state, "locked");
        v.unlock("disposable master fixture").unwrap();
        assert_eq!(v.draft(2, id).unwrap().text, "unsaved fixture text\n");
        let reference = v.payload().unwrap().draft_index[0].blob;
        let bytes = v.storage.read_blob(reference).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("unsaved fixture"));
        v.change_password("disposable master fixture", "another disposable master")
            .unwrap();
        assert_ne!(reference, v.payload().unwrap().draft_index[0].blob);
        drop(v);
        let mut v = Vault::new(Storage::open(dir.path().join("vault")).unwrap());
        assert!(v.unlock("disposable master fixture").is_err());
        v.unlock("another disposable master").unwrap();
        let loaded = v.draft(1, id).unwrap();
        assert_eq!(loaded.path, b"/note_\xff.txt");
        assert_eq!(loaded.endings, vec![2]);
        assert!(loaded.bom);
        assert_eq!(loaded.text, "unsaved fixture text\n");
        v.remove_draft(1, id).unwrap();
        assert!(v.drafts(1).unwrap().is_empty());
    }
    #[test]
    fn failed_persistence_retains_buffers_and_requires_authentication() {
        let (_dir, mut v) = setup();
        v.storage
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let d = draft();
        let id = d.id;
        assert_eq!(v.lock_drafts(1, vec![d]).unwrap().state, "lock_failed");
        assert!(v.authorize(2).is_err());
        assert!(v.unlock("disposable master fixture").is_err());
        assert!(v.recover_lock("wrong", true).is_err());
        assert_eq!(v.status().state, "lock_failed");
        assert_eq!(
            v.pending.as_ref().unwrap()[0].text,
            "unsaved fixture text\n"
        );
        v.storage
            .fail_writes
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            v.recover_lock("disposable master fixture", false)
                .unwrap()
                .state,
            "unlocked"
        );
        assert_eq!(v.draft(2, id).unwrap().text, "unsaved fixture text\n");
        v.storage
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        v.lock_drafts(2, vec![draft()]).unwrap();
        assert_eq!(
            v.recover_lock("disposable master fixture", true)
                .unwrap()
                .state,
            "locked"
        );
        assert!(v.open.is_none());
        assert!(v.pending.is_none());
    }
    #[test]
    fn tampered_blob_never_recovers_or_saves_remotely() {
        let (dir, mut v) = setup();
        let d = draft();
        let id = d.id;
        v.lock_drafts(1, vec![d]).unwrap();
        v.unlock("disposable master fixture").unwrap();
        let blob = v.payload().unwrap().draft_index[0].blob;
        let path = dir.path().join("vault/drafts").join(blob.to_string());
        let mut bytes = std::fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(v.draft(2, id).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn failed_rotation_keeps_old_published_draft_generation() {
        let (_dir, mut v) = setup();
        let draft = draft();
        let id = draft.id;
        v.lock_drafts(1, vec![draft]).unwrap();
        v.unlock("disposable master fixture").unwrap();
        let before = v.storage.read().unwrap();
        let blob = v.payload().unwrap().draft_index[0].blob;
        v.storage
            .fail_writes
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            v.change_password("disposable master fixture", "another disposable master")
                .is_err()
        );
        assert_eq!(v.storage.read().unwrap(), before);
        assert_eq!(v.payload().unwrap().draft_index[0].blob, blob);
        assert_eq!(v.draft(2, id).unwrap().text, "unsaved fixture text\n");
        v.storage
            .fail_writes
            .store(false, std::sync::atomic::Ordering::SeqCst);
        v.lock();
        assert!(v.unlock("another disposable master").is_err());
        v.unlock("disposable master fixture").unwrap();
        assert_eq!(v.draft(3, id).unwrap().text, "unsaved fixture text\n");
    }
}
