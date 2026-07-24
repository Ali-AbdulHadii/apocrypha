//! Apocrypha desktop shell.
//!
//! Tauri is a thin adapter over `apoc-*`: it owns no mod-management logic, so the
//! UI layer can be replaced without touching the engines.

mod commands;
mod nexus_cmds;
mod state;

use state::AppState;

async fn tokio_sleep() {
    std::thread::sleep(std::time::Duration::from_millis(600));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = match AppState::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fatal: could not open application state: {e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        // Registered first so a second launch (which is how Linux delivers a
        // deep link) hands the URL to the running window instead of opening a
        // new one.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::{Emitter, Manager};
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
            if let Some(url) = argv.iter().find(|a| a.starts_with("nxm://")) {
                let _ = app.emit("nxm-url", url.clone());
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::list_games,
            commands::detect_game,
            commands::set_game_path,
            commands::analyze_archive,
            commands::import_mod,
            commands::list_mods,
            commands::set_mod_enabled,
            commands::set_mod_selection,
            commands::preview_deploy,
            commands::deploy,
            commands::rollback_last,
            commands::setup_loader,
            commands::get_settings,
            commands::set_game_db_source,
            commands::list_profiles,
            commands::create_profile,
            commands::switch_profile,
            commands::set_mod_order,
            commands::preview_from_archive,
            commands::preview_from_mod,
            commands::steam_diagnostics,
            nexus_cmds::nexus_status,
            nexus_cmds::set_download_source,
            nexus_cmds::set_nexus_api_key,
            nexus_cmds::register_nxm_handler,
            nexus_cmds::unregister_nxm_handler,
            nexus_cmds::parse_nxm_link,
            nexus_cmds::open_mod_page,
            nexus_cmds::download_from_nxm,
        ])
        .setup(|app| {
            use tauri::{Emitter, Manager};
            // The very first launch can itself be the protocol invocation, so
            // the argument list is checked once at startup too.
            if let Some(url) = std::env::args().find(|a| a.starts_with("nxm://")) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Give the window a moment to mount its listener.
                    tokio_sleep().await;
                    let _ = handle.emit("nxm-url", url);
                });
            }
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Apocrypha");
}
