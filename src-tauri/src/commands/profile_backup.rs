use super::AppState;
use crate::{
    ipc::{AppError, Result},
    services::profiles::backup::{Backup, ImportResult, MAX_BYTES},
};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    sync::Arc,
};
use tauri::{Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    generation: u64,
}
fn parse(value: serde_json::Value) -> Result<Request> {
    serde_json::from_value(value).map_err(|_| AppError::invalid())
}
#[tauri::command]
pub async fn profiles_import(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<ImportResult>> {
    let r = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.vault.lock().map_err(|_| AppError::io())?.authorize(r.generation)?;
        let window = app.get_webview_window("main").ok_or_else(AppError::invalid)?;
        let selected = app.dialog().file()
            .set_title("Importar conexões JSON")
            .set_parent(&window)
            .add_filter("Conexões JSON", &["json"])
            .blocking_pick_file();
        let Some(file) = selected else { return Ok(None); };
        let path = file.into_path().map_err(|_| AppError::invalid())?;
        let file = std::fs::File::open(path).map_err(|_| AppError::io())?;
        if !file.metadata().map_err(|_| AppError::io())?.is_file() {
            return Err(AppError::invalid());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|_| AppError::io())?;
        let backup = Backup::parse(&bytes)?;
        state.vault.lock().map_err(|_| AppError::io())?.authorize(r.generation)?;
        let message = format!(
            "Importar {} conexões e {} pastas?\n\nConexões com IDs já existentes serão ignoradas. Nada será substituído. Senhas, passphrases e chaves privadas não serão importadas.\n\nAs pastas serão preservadas no arquivo exportado. A lista do aplicativo continua sem agrupamento.",
            backup.connections.len(), backup.folders.len()
        );
        let confirmed = app.dialog().message(message)
            .title("Importar conexões")
            .parent(&window)
            .buttons(MessageDialogButtons::OkCancelCustom("Importar".into(), "Cancelar".into()))
            .blocking_show();
        if !confirmed { return Ok(None); }
        let result = state.vault.lock().map_err(|_| AppError::io())?.import_profiles(r.generation, backup)?;
        Ok(Some(result))
    }).await.map_err(|_| AppError::io())?
}

#[tauri::command]
pub async fn profiles_export(
    request: serde_json::Value,
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Option<usize>> {
    let r = parse(request)?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .vault
            .lock()
            .map_err(|_| AppError::io())?
            .authorize(r.generation)?;
        let date: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
        let window = app
            .get_webview_window("main")
            .ok_or_else(AppError::invalid)?;
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Exportar conexões sem credenciais")
            .set_parent(&window)
            .set_file_name(format!("ssh-sessions-{}.json", date.format("%Y-%m-%d")))
            .add_filter("Conexões JSON", &["json"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|_| AppError::invalid())?;
        let vault = state.vault.lock().map_err(|_| AppError::io())?;
        let bytes = vault.export_profiles(r.generation)?;
        let count = vault.profiles(r.generation)?.len();
        atomicwrites::AtomicFile::new(path, atomicwrites::AllowOverwrite)
            .write(|file| -> std::io::Result<()> {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                }
                file.write_all(&bytes)?;
                file.sync_all()
            })
            .map_err(|_| {
                AppError::new(
                    "EXPORT_FAILED",
                    "Não foi possível exportar o JSON. Verifique o destino e suas permissões.",
                )
            })?;
        Ok(Some(count))
    })
    .await
    .map_err(|_| AppError::io())?
}
