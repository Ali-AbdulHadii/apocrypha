//! Nexus Mods commands: protocol registration, API key handling, and turning an
//! `nxm://` link into a downloaded archive.
//!
//! The key constraint, which the UI has to make visible rather than hide:
//! a free Nexus account cannot have a manager request downloads on its behalf.
//! The API refuses unless the request carries a token that only the website
//! mints when the user presses "Mod Manager Download". So for a free account the
//! flow is: open the mod page, let the user press the button, and handle the
//! link that comes back.

use crate::downloads::{self, Download, DownloadState};
use crate::state::AppState;
use apoc_nexus::{DownloadSource, NexusClient, NxmTarget};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{Emitter, State};

type CmdResult<T> = Result<T, String>;

const APP_NAME: &str = "Apocrypha";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const KEY_API: &str = "nexus_api_key";
const KEY_SOURCE: &str = "download_source";
const KEY_USER: &str = "nexus_user_name";
const KEY_PREMIUM: &str = "nexus_is_premium";
const KEY_USER_ID: &str = "nexus_user_id";
const KEY_SSO_SLUG: &str = "nexus_sso_application";
const KEY_SSO_TOKEN: &str = "nexus_sso_token";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusStatusView {
    /// `nexus` or `apocrypha`.
    pub source: String,
    pub has_api_key: bool,
    pub user_name: Option<String>,
    pub is_premium: bool,
    /// Whether this app is registered as the handler for `nxm://` links.
    pub handler_registered: bool,
    pub handler_is_default: bool,
    pub current_handler: Option<String>,
    pub desktop_file: String,
    /// Nexus-issued application id. Browser sign-in cannot work without one.
    pub sso_application: String,
    pub can_sign_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NxmLinkView {
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub has_token: bool,
    pub view_only: bool,
    /// Page to open when a token is needed.
    pub mod_page_url: String,
}

fn client(state: &AppState) -> CmdResult<NexusClient> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let key = store
        .get_setting(KEY_API)
        .map_err(|e| e.to_string())?
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| "No Nexus Mods API key set. Add one in Settings.".to_string())?;
    Ok(NexusClient::new(key, APP_NAME, APP_VERSION))
}

/// Current Nexus configuration and handler registration.
#[tauri::command]
pub fn nexus_status(state: State<AppState>) -> CmdResult<NexusStatusView> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let get = |k: &str| store.get_setting(k).ok().flatten();
    let reg = apoc_nexus::protocol_status();

    Ok(NexusStatusView {
        source: get(KEY_SOURCE).unwrap_or_else(|| DownloadSource::Nexus.as_str().to_string()),
        has_api_key: get(KEY_API).is_some_and(|k| !k.trim().is_empty()),
        user_name: get(KEY_USER),
        is_premium: get(KEY_PREMIUM).as_deref() == Some("true"),
        handler_registered: reg.installed,
        handler_is_default: reg.is_default,
        current_handler: reg.current_handler,
        desktop_file: reg.desktop_file.display().to_string(),
        sso_application: get(KEY_SSO_SLUG).unwrap_or_default(),
        can_sign_in: get(KEY_SSO_SLUG).is_some_and(|s| !s.trim().is_empty()),
    })
}

/// Choose where downloads come from.
#[tauri::command]
pub fn set_download_source(state: State<AppState>, source: String) -> CmdResult<NexusStatusView> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_SOURCE, DownloadSource::parse(&source).as_str())
            .map_err(|e| e.to_string())?;
    }
    nexus_status(state)
}

/// Save an API key after checking it against the account endpoint.
///
/// The key is stored locally and never sent anywhere but Nexus. Validation also
/// tells us whether the account is premium, which decides whether downloads can
/// start without a browser round trip.
#[tauri::command]
pub fn set_nexus_api_key(state: State<AppState>, api_key: String) -> CmdResult<NexusStatusView> {
    let trimmed = api_key.trim().to_string();

    if trimmed.is_empty() {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        for k in [KEY_API, KEY_USER, KEY_PREMIUM, KEY_USER_ID, KEY_SSO_TOKEN] {
            store.set_setting(k, "").map_err(|e| e.to_string())?;
        }
        drop(store);
        return nexus_status(state);
    }

    let probe = NexusClient::new(trimmed.clone(), APP_NAME, APP_VERSION);
    let (user, _limits) = probe.validate().map_err(|e| e.to_string())?;

    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.set_setting(KEY_API, &trimmed).map_err(|e| e.to_string())?;
        store.set_setting(KEY_USER, &user.name).map_err(|e| e.to_string())?;
        store
            .set_setting(KEY_PREMIUM, if user.is_premium { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        store
            .set_setting(KEY_USER_ID, &user.user_id.to_string())
            .map_err(|e| e.to_string())?;
    }
    nexus_status(state)
}

/// Sign in through the browser instead of pasting a key.
///
/// Nexus issues the application id that this flow needs, and only Nexus can
/// issue it. Until Apocrypha has one, this returns a clear error rather than
/// failing obscurely, and pasting a personal key remains available.
#[tauri::command]
pub fn nexus_sign_in(app: tauri::AppHandle, state: State<AppState>) -> CmdResult<NexusStatusView> {
    let slug = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .get_setting(KEY_SSO_SLUG)
            .map_err(|e| e.to_string())?
            .unwrap_or_default()
    };

    let previous = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .get_setting(KEY_SSO_TOKEN)
            .map_err(|e| e.to_string())?
            .filter(|t| !t.is_empty())
    };

    // The browser step happens partway through, so the observer opens the page
    // and tells the interface what is going on while the socket stays open.
    struct Opener(tauri::AppHandle);
    impl apoc_nexus::SsoObserver for Opener {
        fn awaiting_approval(&self, url: &str) {
            use tauri::Emitter;
            let _ = self.0.emit("nexus-sso-awaiting", url.to_string());
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
    }

    let result = apoc_nexus::sign_in(&slug, previous.as_deref(), &Opener(app))
        .map_err(|e| e.to_string())?;

    // Validate straight away so the stored account details are real rather than
    // assumed from a successful handshake.
    let probe = NexusClient::new(result.api_key.clone(), APP_NAME, APP_VERSION);
    let (user, _) = probe.validate().map_err(|e| e.to_string())?;

    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_API, &result.api_key)
            .map_err(|e| e.to_string())?;
        store.set_setting(KEY_USER, &user.name).map_err(|e| e.to_string())?;
        store
            .set_setting(KEY_PREMIUM, if user.is_premium { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        store
            .set_setting(KEY_USER_ID, &user.user_id.to_string())
            .map_err(|e| e.to_string())?;
        if let Some(token) = result.connection_token {
            store
                .set_setting(KEY_SSO_TOKEN, &token)
                .map_err(|e| e.to_string())?;
        }
    }
    nexus_status(state)
}

/// Set the Nexus-issued application id used for browser sign-in.
#[tauri::command]
pub fn set_sso_application(state: State<AppState>, slug: String) -> CmdResult<NexusStatusView> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_SSO_SLUG, slug.trim())
            .map_err(|e| e.to_string())?;
    }
    nexus_status(state)
}

/// Register this application as the system handler for `nxm://` links.
#[tauri::command]
pub fn register_nxm_handler(state: State<AppState>) -> CmdResult<NexusStatusView> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    apoc_nexus::register(&exe).map_err(|e| e.to_string())?;
    nexus_status(state)
}

#[tauri::command]
pub fn unregister_nxm_handler(state: State<AppState>) -> CmdResult<NexusStatusView> {
    apoc_nexus::unregister().map_err(|e| e.to_string())?;
    nexus_status(state)
}

/// Parse an incoming link so the UI can describe it before acting.
#[tauri::command]
pub fn parse_nxm_link(url: String) -> CmdResult<NxmLinkView> {
    let link = apoc_nexus::parse_nxm(&url).map_err(|e| e.to_string())?;
    let (mod_id, file_id) = match &link.target {
        NxmTarget::Mod { mod_id, file_id } => (*mod_id, *file_id),
        NxmTarget::Collection { .. } => {
            return Err("Collections are not supported yet.".to_string())
        }
    };
    Ok(NxmLinkView {
        mod_page_url: apoc_nexus::mod_page_url(&link.domain, mod_id, Some(file_id)),
        domain: link.domain.clone(),
        mod_id,
        file_id,
        has_token: link.has_token(),
        view_only: link.view_only,
    })
}

/// Open a mod page in the browser so the user can press "Mod Manager Download".
#[tauri::command]
pub fn open_mod_page(domain: String, mod_id: u64, file_id: Option<u64>) -> CmdResult<String> {
    let url = apoc_nexus::mod_page_url(&domain, mod_id, file_id);
    open_external(&url)?;
    Ok(url)
}

fn open_external(url: &str) -> CmdResult<()> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open the browser: {e}"))
}

/// Everything in the download queue, newest first.
#[tauri::command]
pub fn list_downloads(state: State<AppState>) -> CmdResult<Vec<Download>> {
    Ok(state.downloads.list(&downloads_dir(&state)))
}

/// Ask a running transfer to stop. The partial file is discarded.
#[tauri::command]
pub fn cancel_download(state: State<AppState>, id: String) -> CmdResult<()> {
    state.downloads.request_cancel(&id);
    Ok(())
}

/// Drop an entry from the queue, deleting the archive if it is on disk.
///
/// This is the same action whether the file was downloaded here or found in the
/// folder, because from the user's side there is no difference between the two.
#[tauri::command]
pub fn remove_download(state: State<AppState>, id: String) -> CmdResult<()> {
    let dir = downloads_dir(&state);
    let path = state
        .downloads
        .get(&id)
        .map(|d| PathBuf::from(d.path))
        .or_else(|| {
            state
                .downloads
                .list(&dir)
                .into_iter()
                .find(|d| d.id == id)
                .map(|d| PathBuf::from(d.path))
        });

    state.downloads.request_cancel(&id);
    state.downloads.forget(&id);

    if let Some(p) = path {
        // Only ever delete inside our own folder, never wherever a path happens
        // to point.
        if p.starts_with(&dir) && p.is_file() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Resolve an `nxm://` link and start downloading it in the background.
///
/// Returns as soon as the transfer is queued rather than when it finishes, so
/// the interface stays usable during a large file. Progress arrives as
/// `download-changed` events carrying the whole updated entry.
#[tauri::command]
pub fn start_nxm_download(
    app: tauri::AppHandle,
    state: State<AppState>,
    url: String,
) -> CmdResult<Download> {
    let link = apoc_nexus::parse_nxm(&url).map_err(|e| e.to_string())?;
    let (mod_id, file_id) = link
        .mod_ids()
        .ok_or_else(|| "Collections are not supported yet.".to_string())?;

    if link.view_only {
        return Err("That link only asks to show the mod, not download it.".into());
    }

    // A token bound to another account will be refused by the API, so say so
    // here rather than surfacing a confusing 400.
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        if let (Some(link_user), Some(stored)) = (
            link.user_id,
            store.get_setting(KEY_USER_ID).ok().flatten(),
        ) {
            if stored.parse::<u64>().ok() == Some(link_user) || stored.is_empty() {
                // Same account, or we do not know yet. Continue.
            } else {
                return Err(
                    "That download link was created for a different Nexus Mods account."
                        .to_string(),
                );
            }
        }
    }

    let client = client(&state)?;
    let token = match (link.key.as_deref(), link.expires) {
        (Some(k), Some(e)) => Some((k, e)),
        _ => None,
    };

    let (links, _limits) = client
        .download_link(&link.domain, mod_id, file_id, token)
        .map_err(|e| e.to_string())?;
    let cdn = links
        .first()
        .ok_or_else(|| "Nexus Mods returned no download location.".to_string())?;

    let file_name = client
        .file_info(&link.domain, mod_id, file_id)
        .map(|(info, _)| info.file_name)
        .unwrap_or_else(|_| format!("nexus-{mod_id}-{file_id}.zip"));

    let dest_dir = downloads_dir(&state);
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(downloads::safe_name(&file_name));

    // A link delivered twice must not start a second writer for the same file.
    let entry = match state.downloads.begin(&file_name, &dest, "Nexus Mods") {
        downloads::Begin::Started(d) => d,
        downloads::Begin::AlreadyRunning(d) => return Ok(d),
    };
    let queue = state.downloads.clone();
    let id = entry.id.clone();
    let uri = cdn.uri.clone();

    // A dedicated thread rather than the async runtime: `ureq` is blocking, and
    // this keeps the transfer off every pool the rest of the app shares.
    std::thread::spawn(move || {
        let emit = {
            let app = app.clone();
            Box::new(move |d: &Download| {
                let _ = app.emit("download-changed", d.clone());
            }) as downloads::OnProgress
        };

        match downloads::fetch(&queue, &id, &uri, &dest, &emit) {
            Ok(_) => {}
            Err(e) => {
                let cancelled = e == "cancelled";
                if let Some(d) = queue.update(&id, |d| {
                    d.state = if cancelled {
                        DownloadState::Cancelled
                    } else {
                        DownloadState::Failed
                    };
                    d.error = (!cancelled).then(|| e.clone());
                }) {
                    let _ = app.emit("download-changed", d);
                }
            }
        }
    });

    Ok(entry)
}

fn downloads_dir(state: &AppState) -> PathBuf {
    state.paths.root().join("downloads")
}
