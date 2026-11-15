mod adapters;
mod commands;
mod ipc;
mod services;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .menu(|app| {
            let menu = tauri::menu::Menu::default(app)?;
            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{MenuItem, PredefinedMenuItem, Submenu};
                // Cocoa's predefined Quit sends terminate: directly and bypasses
                // the runtime's ExitRequested guard. Route Cmd+Q through the UI.
                let quit = MenuItem::with_id(
                    app,
                    "quit-reviewed",
                    "Sair do unny-ssh",
                    true,
                    Some("CmdOrCtrl+Q"),
                )?;
                let application = Submenu::with_items(
                    app,
                    "unny-ssh",
                    true,
                    &[
                        &PredefinedMenuItem::about(app, None, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::services(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &PredefinedMenuItem::hide(app, None)?,
                        &PredefinedMenuItem::hide_others(app, None)?,
                        &PredefinedMenuItem::separator(app)?,
                        &quit,
                    ],
                )?;
                menu.remove_at(0)?;
                menu.insert(&application, 0)?;
            }
            Ok(menu)
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "quit-reviewed" {
                let _ = app.emit_to("main", "app-close-requested", ());
            }
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                let state = window.state::<Arc<commands::AppState>>().inner().clone();
                let generation = state.access.load(std::sync::atomic::Ordering::SeqCst);
                if generation == 0 {
                    return;
                }
                let paths = paths.clone();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let result = services::transfers::Transfers::open_sources(paths);
                    if state.authorize(generation).is_err() {
                        return;
                    }
                    match result.and_then(|sources| {
                        state
                            .transfers
                            .lock()
                            .map_err(|_| ipc::AppError::io())?
                            .selected(generation, sources)
                    }) {
                        Ok(selections) => {
                            let _ = app.emit_to("main", "local-files-selected", selections);
                        }
                        Err(error) => {
                            let _ = app.emit_to("main", "local-files-error", error);
                        }
                    }
                });
            }
        })
        .setup(|app| {
            adapters::power::watch(app.handle().clone());
            let dir = app.path().app_local_data_dir()?.join("vault");
            #[cfg(debug_assertions)]
            let dir = std::env::var_os("UNNY_SMOKE_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(dir);
            let storage = adapters::storage::Storage::open(dir).map_err(|_| {
                std::io::Error::other("Não foi possível abrir o armazenamento protegido.")
            })?;
            let state = Arc::new(commands::AppState {
                vault: Mutex::new(services::vault::Vault::new(storage)),
                access: std::sync::atomic::AtomicU64::new(0),
                streams: Mutex::default(),
                confirmations: Mutex::default(),
                sessions: Mutex::default(),
                imports: Mutex::default(),
                profile_deletes: Mutex::default(),
                docker: Mutex::default(),
                files: Mutex::default(),
                transfers: Mutex::default(),
            });
            let weak = Arc::downgrade(&state);
            tauri::async_runtime::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(30));
                loop {
                    timer.tick().await;
                    let Some(state) = weak.upgrade() else { break };
                    if let Ok(mut imports) = state.imports.lock() {
                        imports.cleanup();
                    }
                    if let Ok(mut transfers) = state.transfers.lock() {
                        transfers.cleanup();
                    }
                }
            });
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_status,
            commands::docker::docker_inventory,
            commands::docker::docker_inspect,
            commands::docker::docker_prepare,
            commands::docker::docker_apply,
            commands::docker::docker_logs,
            commands::docker::docker_stream_action,
            commands::docker::docker_exec,
            commands::docker::compose_list,
            commands::docker::compose_register,
            commands::docker::compose_remove,
            commands::docker::compose_status,
            commands::docker::compose_prepare,
            commands::docker::compose_apply,
            commands::lifecycle::workspace_lock,
            commands::lifecycle::workspace_recover,
            commands::lifecycle::drafts_list,
            commands::lifecycle::draft_read,
            commands::lifecycle::draft_remove,
            commands::lifecycle::draft_restore,
            commands::transfers::transfer_select_files,
            commands::transfers::transfer_select_destination,
            commands::transfers::transfer_upload,
            commands::transfers::transfer_download,
            commands::transfers::transfers_list,
            commands::transfers::transfer_action,
            commands::transfers::finish_window_close,
            commands::files::files_list,
            commands::files::files_terminal,
            commands::files::files_mutate,
            commands::files::document_open,
            commands::files::document_save,
            commands::files::files_release,
            commands::connections::clipboard_write,
            commands::connections::clipboard_read,
            commands::connections::profiles_list,
            commands::connections::profile_put,
            commands::connections::profile_duplicate,
            commands::connections::profile_delete_prepare,
            commands::connections::profile_delete,
            commands::connections::key_import,
            commands::connections::session_connect,
            commands::connections::sessions_list,
            commands::connections::session_confirm_host,
            commands::connections::session_disconnect,
            commands::connections::session_close,
            commands::connections::terminal_open,
            commands::connections::terminal_action,
            commands::reset_challenge,
            commands::vault_action,
            commands::domain_probe,
            commands::stream_open,
            commands::stream_ack,
            commands::stream_cancel
        ])
        .manage(commands::transfers::ExitApproval::default())
        .build(tauri::generate_context!())
        .expect("Não foi possível iniciar unny-ssh")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !app
                    .state::<commands::transfers::ExitApproval>()
                    .0
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    api.prevent_exit();
                    let _ = app.emit_to("main", "app-close-requested", ());
                }
            }
        });
}
