use super::*;
use crate::adapters::sftp::CHUNK;
use sha2::{Digest, Sha256};
pub const EDIT_LIMIT: usize = 5 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revision {
    pub size: u64,
    pub mtime: Option<u32>,
    pub sha256: String,
}
#[derive(Clone)]
pub struct DocumentMeta {
    pub draft_id: Option<Uuid>,
    pub path: Vec<u8>,
    pub revision: Revision,
    pub bom: bool,
    pub endings: Vec<u8>,
    pub preferred: u8,
    pub permissions: Option<u32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: Uuid,
    pub entry: RemoteEntry,
    pub text: String,
    pub revision: Revision,
    pub bom: bool,
    pub line_endings: String,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenReply {
    Document { document: Document },
    Restricted { entry: RemoteEntry, reason: String },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveReply {
    Saved {
        revision: Revision,
    },
    Conflict {
        token: Uuid,
        current: Option<Revision>,
        missing: bool,
    },
    NonAtomic {
        token: Uuid,
    },
    Failed {
        error: AppError,
        residue: Option<String>,
    },
}
fn revision(bytes: &[u8], attrs: &FileAttributes) -> Revision {
    Revision {
        size: bytes.len() as u64,
        mtime: attrs.mtime,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
pub fn decode_text(bytes: &[u8]) -> Result<(String, bool, Vec<u8>, u8)> {
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let body = if bom { &bytes[3..] } else { bytes };
    let text = std::str::from_utf8(body).map_err(|_| {
        AppError::new(
            "UNSUPPORTED_ENCODING",
            "O arquivo não é UTF-8. Faça download para abrir em outro aplicativo.",
        )
    })?;
    if body
        .iter()
        .any(|b| *b == 0 || (*b < 32 && !matches!(*b, 9 | 10 | 12 | 13)))
    {
        return Err(AppError::new(
            "BINARY_FILE",
            "O arquivo contém dados binários. Faça download para abri-lo.",
        ));
    }
    let mut endings = Vec::new();
    let mut index = 0;
    let mut counts = [0usize; 4];
    while index < body.len() {
        let code = match body[index] {
            b'\r' => {
                if body.get(index + 1) == Some(&b'\n') {
                    index += 1;
                    2
                } else {
                    3
                }
            }
            b'\n' => 1,
            _ => 0,
        };
        if code != 0 {
            counts[code as usize] += 1;
            endings.push(code);
        }
        index += 1;
    }
    let preferred = (1..=3)
        .max_by_key(|i| counts[*i])
        .filter(|i| counts[*i] > 0)
        .unwrap_or(1) as u8;
    Ok((
        text.replace("\r\n", "\n").replace('\r', "\n"),
        bom,
        endings,
        preferred,
    ))
}
pub fn encode_text(text: &str, meta: &DocumentMeta) -> Result<Vec<u8>> {
    if text.len() > EDIT_LIMIT || text.as_bytes().contains(&0) {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "O conteúdo deve ser texto de até 5 MiB, sem NUL.",
        ));
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = Vec::with_capacity(normalized.len() + 3);
    if meta.bom {
        result.extend([0xef, 0xbb, 0xbf]);
    }
    let mut line = 0;
    for byte in normalized.bytes() {
        if byte == b'\n' {
            match meta.endings.get(line).copied().unwrap_or(meta.preferred) {
                2 => result.extend(b"\r\n"),
                3 => result.push(b'\r'),
                _ => result.push(b'\n'),
            }
            line += 1;
        } else {
            result.push(byte);
        }
    }
    if result.len() > EDIT_LIMIT {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "O arquivo codificado excede 5 MiB.",
        ));
    }
    Ok(result)
}
pub async fn read_bounded(wire: &mut Wire, path: &[u8]) -> Result<(Vec<u8>, FileAttributes)> {
    let attrs = wire.stat(path, false).await?;
    if kind(&attrs) != "file" {
        return Err(AppError::new(
            "UNSUPPORTED",
            "Somente arquivos regulares podem ser editados. Links não são seguidos.",
        ));
    }
    if attrs.size.is_some_and(|size| size > EDIT_LIMIT as u64) {
        return Err(AppError::new(
            "FILE_TOO_LARGE",
            "O arquivo excede 5 MiB. Faça download para editá-lo localmente.",
        ));
    }
    let handle = wire.open(path, 1, None).await?;
    let result = async {
        let before = wire.fstat(&handle).await?;
        if kind(&before) != "file" {
            return Err(AppError::invalid());
        }
        let mut bytes = Vec::new();
        loop {
            let chunk = wire.read(&handle, bytes.len() as u64).await?;
            if chunk.is_empty() {
                break;
            }
            if bytes.len() + chunk.len() > EDIT_LIMIT {
                return Err(AppError::new("FILE_TOO_LARGE", "O arquivo excede 5 MiB."));
            }
            bytes.extend(chunk);
        }
        let after = wire.fstat(&handle).await?;
        if Stamp::from(&before) != Stamp::from(&after)
            || after.size.is_some_and(|size| size != bytes.len() as u64)
        {
            return Err(AppError::new(
                "CONFLICT",
                "O arquivo mudou durante a leitura. Tente novamente.",
            ));
        }
        Ok((bytes, after))
    }
    .await;
    let closed = wire.close(&handle).await;
    match result {
        Ok(data) => {
            closed?;
            Ok(data)
        }
        Err(error) => Err(error),
    }
}
pub async fn open_document(ctx: &Context, entry: Uuid) -> Result<OpenReply> {
    if ctx.cancel.is_cancelled() {
        return Err(sessions::expired());
    }
    let path = ctx.path(entry)?;
    let mut wire = ctx.wire.lock().await;
    let attrs = wire.stat(&path, false).await?;
    let entry = ctx.entry(path.clone(), &attrs)?;
    if kind(&attrs) != "file" || attrs.size.is_some_and(|size| size > EDIT_LIMIT as u64) {
        return Ok(OpenReply::Restricted{entry,reason:"Somente texto UTF-8 regular de até 5 MiB pode ser editado. Use o download para outros arquivos; links não são seguidos.".into()});
    }
    let (bytes, attrs) = read_bounded(&mut wire, &path).await?;
    let (text, bom, endings, preferred) = match decode_text(&bytes) {
        Ok(text) => text,
        Err(error) => {
            return Ok(OpenReply::Restricted {
                entry,
                reason: error.message.into(),
            });
        }
    };
    let revision = revision(&bytes, &attrs);
    let line_endings = if endings.iter().any(|e| *e != preferred) {
        "Misto"
    } else {
        match preferred {
            2 => "CRLF",
            3 => "CR",
            _ => "LF",
        }
    }
    .to_owned();
    let id = Uuid::new_v4();
    let mut docs = ctx.documents.lock().map_err(|_| AppError::io())?;
    if docs.len() >= 16 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "Feche um documento desta sessão antes de abrir outro (limite: 16).",
        ));
    }
    docs.insert(
        id,
        DocumentMeta {
            draft_id: None,
            path,
            revision: revision.clone(),
            bom,
            endings,
            preferred,
            permissions: attrs.permissions,
        },
    );
    Ok(OpenReply::Document {
        document: Document {
            id,
            entry,
            text,
            revision,
            bom,
            line_endings,
        },
    })
}
async fn current(wire: &mut Wire, path: &[u8]) -> Result<Option<Revision>> {
    match read_bounded(wire, path).await {
        Ok((bytes, attrs)) => Ok(Some(revision(&bytes, &attrs))),
        Err(error) if error.code == "NOT_FOUND" => Ok(None),
        Err(error) => Err(error),
    }
}
pub async fn save_document(
    ctx: &Context,
    id: Uuid,
    expected: Revision,
    text: String,
    token: Option<Uuid>,
) -> Result<SaveReply> {
    if ctx.cancel.is_cancelled() {
        return Err(sessions::expired());
    }
    let meta = ctx
        .documents
        .lock()
        .map_err(|_| AppError::io())?
        .get(&id)
        .cloned()
        .ok_or_else(sessions::unknown)?;
    if meta.revision != expected {
        return Err(AppError::new(
            "CONFLICT",
            "A revisão desta aba expirou. Recarregue antes de salvar.",
        ));
    }
    let bytes = encode_text(&text, &meta)?;
    let mut wire = ctx.wire.lock().await;
    let observed = current(&mut wire, &meta.path).await?;
    let mut allow_conflict = false;
    let mut fallback = false;
    if let Some(token) = token {
        let PermitAction::Save {
            document,
            observed: approved,
            fallback: approved_fallback,
        } = ctx.consume(token)?
        else {
            return Err(AppError::invalid());
        };
        if document != id {
            return Err(AppError::invalid());
        }
        if approved == observed {
            allow_conflict = true;
            fallback = approved_fallback;
        }
    }
    if observed.as_ref() != Some(&expected) && !allow_conflict {
        let token = ctx.permit(PermitAction::Save {
            document: id,
            observed: observed.clone(),
            fallback: false,
        })?;
        return Ok(SaveReply::Conflict {
            token,
            missing: observed.is_none(),
            current: observed,
        });
    }
    if !wire.atomic_rename && !fallback {
        let token = ctx.permit(PermitAction::Save {
            document: id,
            observed: observed.clone(),
            fallback: true,
        })?;
        return Ok(SaveReply::NonAtomic { token });
    }
    let temporary = join(
        &parent(&meta.path).ok_or_else(AppError::invalid)?,
        format!(".unny-edit-{}.part", Uuid::new_v4()).as_bytes(),
    )?;
    let saved = async {
        let handle = wire.open(&temporary, 2 | 8 | 32, Some(0o600)).await?;
        let written = async {
            for (index, chunk) in bytes.chunks(CHUNK).enumerate() {
                wire.write(&handle, (index * CHUNK) as u64, chunk).await?;
            }
            if let Some(mode) = meta.permissions {
                wire.set_permissions(&handle, mode).await?;
            }
            wire.sync(&handle).await
        }
        .await;
        let closed = wire.close(&handle).await;
        written?;
        closed?;
        // Compare again after writing the sibling. No SFTP v3 compare-and-swap exists.
        if current(&mut wire, &meta.path).await? != observed {
            return Err(AppError::new(
                "CONFLICT",
                "O arquivo mudou durante a gravação do temporário. Nenhuma substituição foi feita.",
            ));
        }
        if wire.atomic_rename {
            wire.rename(&temporary, &meta.path, true).await?;
        } else {
            let target = wire.open(&meta.path, 2 | 8 | 16, None).await?;
            let write = async {
                for (index, chunk) in bytes.chunks(CHUNK).enumerate() {
                    wire.write(&target, (index * CHUNK) as u64, chunk).await?;
                }
                if let Some(mode) = meta.permissions {
                    wire.set_permissions(&target, mode).await?;
                }
                wire.sync(&target).await
            }
            .await;
            let close = wire.close(&target).await;
            write?;
            close?;
            wire.remove(&temporary, false).await?;
        }
        let attrs = wire.stat(&meta.path, false).await?;
        Ok::<_, AppError>(revision(&bytes, &attrs))
    }
    .await;
    match saved {
        Ok(revision) => {
            let (_, _, endings, preferred) = decode_text(&bytes)?;
            if let Some(doc) = ctx
                .documents
                .lock()
                .map_err(|_| AppError::io())?
                .get_mut(&id)
            {
                doc.revision = revision.clone();
                doc.endings = endings;
                doc.preferred = preferred;
            }
            Ok(SaveReply::Saved { revision })
        }
        Err(error) => {
            let cleaned =
                tokio::time::timeout(Duration::from_secs(2), wire.remove(&temporary, false)).await;
            let residue = if matches!(cleaned, Ok(Ok(())))
                || matches!(&cleaned,Ok(Err(e))if e.code=="NOT_FOUND")
            {
                None
            } else {
                Some(display(&temporary))
            };
            Ok(SaveReply::Failed { error, residue })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_bom_and_mixed_endings_and_new_lines() {
        for original in [
            b"\xef\xbb\xbfalpha\r\nbeta\ngamma\r".as_slice(),
            b"",
            b"utf8 \xc3\xa7\n",
        ] {
            let (text, bom, endings, preferred) = decode_text(original).unwrap();
            let meta = DocumentMeta {
                draft_id: None,
                path: b"/test".to_vec(),
                revision: revision(original, &FileAttributes::default()),
                bom,
                endings,
                preferred,
                permissions: Some(0o640),
            };
            assert_eq!(encode_text(&text, &meta).unwrap(), original);
        }
        let (text, bom, endings, preferred) = decode_text(b"a\r\nb\r\n").unwrap();
        let meta = DocumentMeta {
            draft_id: None,
            path: vec![],
            revision: revision(b"", &FileAttributes::default()),
            bom,
            endings,
            preferred,
            permissions: None,
        };
        assert_eq!(
            encode_text(&(text + "new\n"), &meta).unwrap(),
            b"a\r\nb\r\nnew\r\n"
        );
        assert!(encode_text(&"x".repeat(EDIT_LIMIT + 1), &meta).is_err());
        assert!(decode_text(b"binary\0").is_err());
        assert!(decode_text(b"\xff").is_err());
    }
    #[test]
    fn hash_detects_equal_size_and_time_changes() {
        let attrs = FileAttributes {
            mtime: Some(123),
            ..Default::default()
        };
        assert_ne!(revision(b"one", &attrs), revision(b"two", &attrs));
    }
}

pub fn capture(
    ctx: &Context,
    id: Uuid,
    text: String,
    identity: crate::services::vault::drafts::Identity,
) -> Result<crate::services::vault::drafts::Draft> {
    let docs = ctx.documents.lock().map_err(|_| AppError::io())?;
    let meta = docs.get(&id).ok_or_else(AppError::invalid)?;
    Ok(crate::services::vault::drafts::Draft {
        id: meta.draft_id.unwrap_or(id),
        identity,
        name: display(basename(&meta.path)),
        path: meta.path.clone(),
        text,
        revision: meta.revision.clone(),
        bom: meta.bom,
        endings: meta.endings.clone(),
        preferred: meta.preferred,
        permissions: meta.permissions,
    })
}
pub async fn restore(
    ctx: &Context,
    draft: crate::services::vault::drafts::Draft,
) -> Result<Document> {
    let attrs = ctx
        .wire
        .lock()
        .await
        .maybe_stat(&draft.path)
        .await?
        .unwrap_or(FileAttributes {
            permissions: Some(0o100000 | draft.permissions.unwrap_or(0o600)),
            ..Default::default()
        });
    if kind(&attrs) != "file" {
        return Err(AppError::new(
            "UNSUPPORTED",
            "O destino deixou de ser um arquivo regular. O rascunho continua protegido.",
        ));
    }
    let entry = ctx.entry(draft.path.clone(), &attrs)?;
    let id = Uuid::new_v4();
    let mut docs = ctx.documents.lock().map_err(|_| AppError::io())?;
    if docs.len() >= 16 {
        return Err(AppError::new(
            "LIMIT_REACHED",
            "Feche um documento desta sessão.",
        ));
    }
    docs.insert(
        id,
        DocumentMeta {
            draft_id: Some(draft.id),
            path: draft.path.clone(),
            revision: draft.revision.clone(),
            bom: draft.bom,
            endings: draft.endings.clone(),
            preferred: draft.preferred,
            permissions: draft.permissions,
        },
    );
    Ok(Document {
        id,
        entry,
        text: draft.text.clone(),
        revision: draft.revision.clone(),
        bom: draft.bom,
        line_endings: "Rascunho recuperado".into(),
    })
}
