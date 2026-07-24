//! Apocrypha desktop shell.
//!
//! Tauri is a thin adapter over `apoc-*`: it owns no mod-management logic, so the
//! UI layer can be replaced without touching the engines.

mod commands;
mod state;

use state::AppState;

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Apocrypha");
}
