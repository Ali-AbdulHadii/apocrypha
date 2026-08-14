//! Apocrypha desktop shell.
//!
//! Tauri is a thin adapter over `apoc-*`: it owns no mod-management logic, so the
//! UI layer can be replaced without touching the engines.

mod account_cmds;
mod app_update;
mod commands;
mod deploy_cmds;
mod downloads;
mod gamedb;
mod nexus_cmds;
mod state;

use state::AppState;

async fn tokio_sleep() {
    std::thread::sleep(std::time::Duration::from_millis(600));
}

/// Whether WebKit's DMABUF renderer should be turned off for this session.
///
/// Split out from the setting of it so the rule is testable: the decision is
/// what matters and the environment is awkward to fake.
#[cfg(target_os = "linux")]
fn should_disable_dmabuf(wayland: bool, already_set: bool) -> bool {
    // Never override a value the user chose. Someone debugging this wants their
    // setting to win, including when they set it to 0.
    wayland && !already_set
}

/// Work around a WebKitGTK crash on Wayland before GTK is initialized.
///
/// On Wayland, recent WebKitGTK dies during window creation with
/// `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display`,
/// and the process exits before any of this application's code runs. It
/// reproduces on Hyprland with WebKitGTK 2.52 and a transparent, undecorated
/// window, which is exactly the window this app asks for — the custom chrome
/// depends on it, so the window is not the thing to change.
///
/// Disabling the DMABUF renderer avoids it. `WEBKIT_DISABLE_COMPOSITING_MODE`
/// and falling back to X11 through `GDK_BACKEND` also avoid it; this one is
/// preferred because it gives up the least — the compositing switch disables
/// more of the rendering path, and XWayland costs native Wayland behaviour
/// including fractional scaling.
///
/// The cost is DMABUF-accelerated buffer sharing in the web view. For a UI that
/// is lists and forms, that is not measurable; a crash on launch is.
///
/// Must run before GTK initializes, which means before `tauri::Builder`.
#[cfg(target_os = "linux")]
fn apply_linux_rendering_workarounds() {
    const KEY: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if should_disable_dmabuf(wayland, std::env::var_os(KEY).is_some()) {
        std::env::set_var(KEY, "1");
    }
}

/// Whether a newer Apocrypha has been released.
///
/// Deliberately a command rather than something done at startup in Rust: the
/// interface decides when to ask and how loudly to say it, and a check that
/// blocks the window appearing would be the wrong trade for information this
/// low-stakes.
#[tauri::command(async)]
fn check_app_update() -> app_update::AppUpdateView {
    app_update::check(env!("CARGO_PKG_VERSION"))
}

/// A protocol invocation found among the process arguments.
enum Link {
    Nxm(String),
    Apocrypha(String),
}

/// The one link in an argument list, if there is exactly one.
///
/// Both ingress paths — the first launch and the second-instance callback —
/// come through here, because a link is chosen by whoever published the page
/// that opened it, and two places doing this slightly differently is how one of
/// them becomes the lenient one.
///
/// An `apocrypha://` argument is validated against the full grammar here rather
/// than merely recognised, so a malformed one never reaches the window at all.
/// The argument must *be* the link: an argument that contains one is not one,
/// since this process can be started with any arguments at all.
fn link_from(args: &[String]) -> Option<Link> {
    for arg in args {
        if arg.starts_with("nxm://") {
            return Some(Link::Nxm(arg.clone()));
        }
        if apoc_apocrypha::protocol::parse(arg).is_ok() {
            return Some(Link::Apocrypha(arg.clone()));
        }
    }
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    apply_linux_rendering_workarounds();

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
            match link_from(&argv) {
                Some(Link::Nxm(url)) => {
                    let _ = app.emit("nxm-url", url);
                }
                Some(Link::Apocrypha(url)) => {
                    let _ = app.emit("apocrypha-url", url);
                }
                None => {}
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::list_games,
            commands::detect_game,
            commands::game_art,
            commands::launch_game,
            commands::game_for_domain,
            commands::set_game_path,
            commands::game_db_status,
            commands::refresh_game_db,
            commands::analyze_archive,
            commands::evaluate_selection,
            commands::import_mod,
            commands::list_mods,
            commands::set_mod_enabled,
            commands::set_mods_enabled,
            commands::set_mod_selection,
            commands::preview_deploy,
            commands::rollback_last,
            deploy_cmds::start_deploy,
            deploy_cmds::cancel_deploy,
            deploy_cmds::list_conflicts,
            deploy_cmds::set_conflict_override,
            deploy_cmds::clear_conflict_override,
            deploy_cmds::conflict_overrides,
            deploy_cmds::verify_deployment,
            deploy_cmds::repair_deployment,
            deploy_cmds::storage_usage,
            deploy_cmds::open_path,
            commands::setup_loader,
            commands::get_settings,
            commands::set_game_db_source,
            commands::set_downloads_dir,
            commands::list_profiles,
            commands::create_profile,
            commands::switch_profile,
            commands::duplicate_profile,
            commands::delete_profile,
            commands::set_mod_order,
            commands::remove_mod,
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
            nexus_cmds::start_nxm_download,
            nexus_cmds::check_mod_updates,
            nexus_cmds::download_mod_update,
            nexus_cmds::list_downloads,
            nexus_cmds::cancel_download,
            nexus_cmds::remove_download,
            nexus_cmds::nexus_sign_in,
            nexus_cmds::set_sso_application,
            check_app_update,
            account_cmds::apocrypha_account,
            account_cmds::start_apocrypha_authorization,
            account_cmds::poll_apocrypha_authorization,
            account_cmds::cancel_apocrypha_authorization,
            account_cmds::sign_out_apocrypha,
            account_cmds::browse_apocrypha_mods,
            account_cmds::apocrypha_games,
            account_cmds::apocrypha_mod_detail,
            account_cmds::apocrypha_download_quota,
            account_cmds::apocrypha_download_file,
            account_cmds::preview_apocrypha_link,
        ])
        .setup(|app| {
            use tauri::{Emitter, Manager};
            // The very first launch can itself be the protocol invocation, so
            // the argument list is checked once at startup too — through the
            // same function, because two ingress paths with two validations is
            // how one of them ends up being the lenient one.
            let args: Vec<String> = std::env::args().collect();
            if let Some(link) = link_from(&args) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Give the window a moment to mount its listener.
                    tokio_sleep().await;
                    let _ = match link {
                        Link::Nxm(url) => handle.emit("nxm-url", url),
                        Link::Apocrypha(url) => handle.emit("apocrypha-url", url),
                    };
                });
            }
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Apocrypha");
}

#[cfg(test)]
mod link_tests {
    use super::{link_from, Link};

    const GOOD: &str =
        "apocrypha://install?game=monster-hunter-wilds&mod=reframework&file=3f2504e0-4f89-41d3-9a0c-0305e82c3301";

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_link_is_found_past_the_executable_name() {
        // argv[0] is always there, so the search cannot start at the first item.
        let found = link_from(&args(&["/usr/bin/apocrypha", GOOD]));
        assert!(matches!(found, Some(Link::Apocrypha(u)) if u == GOOD));
    }

    #[test]
    fn a_malformed_apocrypha_link_never_reaches_the_window() {
        // Validated here, not merely recognised: emitting it and letting the
        // window sort it out would put an attacker-authored string through the
        // event channel first.
        let bad = "apocrypha://install?game=../../etc&mod=m&file=x";
        assert!(link_from(&args(&["/usr/bin/apocrypha", bad])).is_none());
    }

    #[test]
    fn an_ordinary_launch_finds_nothing() {
        assert!(link_from(&args(&["/usr/bin/apocrypha"])).is_none());
        assert!(link_from(&args(&["/usr/bin/apocrypha", "--flag", "value"])).is_none());
    }

    #[test]
    fn nxm_still_works() {
        let url = "nxm://skyrimspecialedition/mods/1/files/2";
        assert!(matches!(link_from(&args(&["x", url])), Some(Link::Nxm(u)) if u == url));
    }

    #[test]
    fn an_argument_that_merely_contains_a_link_is_not_one() {
        let smuggled = format!("--config={GOOD}");
        assert!(link_from(&args(&["x", &smuggled])).is_none());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::should_disable_dmabuf;

    #[test]
    fn wayland_without_an_existing_setting_gets_the_workaround() {
        assert!(should_disable_dmabuf(true, false));
    }

    #[test]
    fn x11_is_left_alone() {
        // The crash is a Wayland protocol error. An X11 session does not need
        // the workaround and should keep the faster rendering path.
        assert!(!should_disable_dmabuf(false, false));
    }

    #[test]
    fn a_value_the_user_already_chose_is_never_overridden() {
        // Including when they set it to 0 to reproduce the crash deliberately.
        assert!(!should_disable_dmabuf(true, true));
        assert!(!should_disable_dmabuf(false, true));
    }
}
