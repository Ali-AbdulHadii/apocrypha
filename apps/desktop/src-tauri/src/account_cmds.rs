//! Signing this installation in to the Apocrypha service.
//!
//! Three commands and no loop: the interface starts a pairing, opens the
//! browser, and polls on its own timer. Waiting belongs to the window, which
//! can stay responsive and let someone cancel; a blocking call here would take
//! the choice away and hold a worker thread for ten minutes.

use apoc_apocrypha::{
    Catalog, CatalogModDetail, CatalogPage, DevicePairing, DownloadQuota, PairingStatus,
    ServiceOrigin,
};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::downloads::{self, Download};
use crate::state::AppState;

type CmdResult<T> = Result<T, String>;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

const KEY_TOKEN: &str = "apocrypha_token";
const KEY_TOKEN_EXPIRES: &str = "apocrypha_token_expires";
const KEY_DEVICE_NAME: &str = "apocrypha_device_name";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApocryphaAccountView {
    pub signed_in: bool,
    /// What this installation called itself when it paired.
    pub device_name: Option<String>,
    pub expires_at: Option<String>,
    pub service_origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingStartedView {
    pub device_code: String,
    /// Grouped for reading. This is what goes on screen next to the browser.
    pub user_code_display: String,
    pub approval_url: String,
    pub expires_in_seconds: i64,
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingPollView {
    /// `pending`, `slowDown`, or `granted`.
    pub status: String,
}

fn origin() -> ServiceOrigin {
    ServiceOrigin::default()
}

/// A name for this machine, for the approval screen to show.
///
/// The hostname rather than the application name: whoever is approving already
/// knows which app is asking and needs to know which computer.
fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
        })
        .map(|h| format!("Apocrypha on {h}"))
        .unwrap_or_else(|| "Apocrypha desktop".to_string())
}

#[tauri::command(async)]
pub fn apocrypha_account(state: State<AppState>) -> CmdResult<ApocryphaAccountView> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let get = |k: &str| store.get_setting(k).ok().flatten();
    Ok(ApocryphaAccountView {
        signed_in: get(KEY_TOKEN).is_some_and(|t| !t.trim().is_empty()),
        device_name: get(KEY_DEVICE_NAME),
        expires_at: get(KEY_TOKEN_EXPIRES),
        service_origin: origin().as_str().to_string(),
    })
}

/// Begins pairing and returns what the window needs to show.
///
/// The device code comes back to the interface because the interface is what
/// polls. It is a secret for the length of one pairing and is never written
/// down: nothing here persists it, and the window drops it when the pairing
/// finishes or is cancelled.
#[tauri::command(async)]
pub fn start_apocrypha_pairing(state: State<AppState>) -> CmdResult<PairingStartedView> {
    let name = default_device_name();
    let pairing = DevicePairing::new(origin(), APP_VERSION);
    let started = pairing.start(&name).map_err(|e| e.to_string())?;

    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_DEVICE_NAME, &name)
            .map_err(|e| e.to_string())?;
    }

    Ok(PairingStartedView {
        approval_url: pairing.approval_url(&started.user_code),
        device_code: started.device_code,
        user_code_display: started.user_code_display,
        expires_in_seconds: started.expires_in_seconds,
        poll_interval_seconds: started.poll_interval_seconds,
    })
}

/// One poll. Stores the token itself when it arrives, so the secret never
/// reaches the interface.
#[tauri::command(async)]
pub fn poll_apocrypha_pairing(
    state: State<AppState>,
    device_code: String,
) -> CmdResult<PairingPollView> {
    let pairing = DevicePairing::new(origin(), APP_VERSION);
    match pairing.poll(&device_code).map_err(|e| e.to_string())? {
        PairingStatus::Pending => Ok(PairingPollView {
            status: "pending".into(),
        }),
        PairingStatus::SlowDown => Ok(PairingPollView {
            status: "slowDown".into(),
        }),
        PairingStatus::Granted { token, expires_at } => {
            let store = state.store.lock().map_err(|_| "state poisoned")?;
            store
                .set_setting(KEY_TOKEN, &token)
                .map_err(|e| e.to_string())?;
            store
                .set_setting(KEY_TOKEN_EXPIRES, &expires_at)
                .map_err(|e| e.to_string())?;
            Ok(PairingPollView {
                status: "granted".into(),
            })
        }
    }
}

/// Forgets the token locally.
///
/// Deliberately not called "revoke": the grant still exists on the service
/// until it is revoked there, and saying otherwise would be a lie about
/// something security-relevant. The interface says so too.
#[tauri::command(async)]
pub fn sign_out_apocrypha(state: State<AppState>) -> CmdResult<ApocryphaAccountView> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_TOKEN, "")
            .map_err(|e| e.to_string())?;
        store
            .set_setting(KEY_TOKEN_EXPIRES, "")
            .map_err(|e| e.to_string())?;
    }
    apocrypha_account(state)
}

/// Browse the service catalogue as this account.
///
/// Returns the service's page verbatim. Nothing is filtered here: what may be
/// seen is the server's decision, and a client-side filter is one that can be
/// switched off.
#[tauri::command(async)]
pub fn browse_apocrypha_mods(
    state: State<AppState>,
    game: Option<String>,
    search: Option<String>,
    page: u32,
) -> CmdResult<CatalogPage> {
    catalog(&state)?
        .mods(game.as_deref(), search.as_deref(), page, 24)
        .map_err(|e| e.to_string())
}

/// One mod with its releases and their files.
///
/// Separate from the listing because it is a second request, and a listing of
/// twenty mods should not fetch twenty sets of files nobody has asked to see.
#[tauri::command(async)]
pub fn apocrypha_mod_detail(
    state: State<AppState>,
    game_slug: String,
    mod_slug: String,
) -> CmdResult<CatalogModDetail> {
    catalog(&state)?
        .mod_detail(&game_slug, &mod_slug)
        .map_err(|e| e.to_string())
}

/// What is left of today's download allowance.
#[tauri::command(async)]
pub fn apocrypha_download_quota(state: State<AppState>) -> CmdResult<DownloadQuota> {
    catalog(&state)?.download_quota().map_err(|e| e.to_string())
}

/// Claim a file from the service and fetch it into the download queue.
///
/// It stops at the queue, exactly like a Nexus download: a finished archive
/// waits until someone chooses to install it. Nothing here touches a game
/// directory, and the install path it feeds is the one that was already there.
#[tauri::command(async)]
pub fn apocrypha_download_file(
    app: tauri::AppHandle,
    state: State<AppState>,
    game_slug: String,
    mod_slug: String,
    file_id: String,
) -> CmdResult<Download> {
    let catalog = catalog(&state)?;

    // The detail is re-read rather than trusting what the window sent. The
    // interface's copy can be minutes old, and the two things taken from here —
    // the expected hash and the file name — are the ones it would be worst to
    // take from a stale or edited client.
    let detail = catalog
        .mod_detail(&game_slug, &mod_slug)
        .map_err(|e| e.to_string())?;

    let file = detail
        .versions
        .iter()
        .flat_map(|v| v.files.iter())
        .find(|f| f.id == file_id)
        .ok_or_else(|| "That file is no longer part of this mod.".to_string())?;

    if !file.is_downloadable() {
        return Err("That file is not ready to download yet.".into());
    }

    let expected_sha = file.sha256.to_ascii_lowercase();
    let ticket = catalog
        .claim_download(&file_id)
        .map_err(|e| e.to_string())?;

    let dest_dir = state.downloads_dir();
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(downloads::safe_name(&ticket.file_name));

    // A second press must not start a second writer for the same file.
    let entry = match state.downloads.begin(&ticket.file_name, &dest, "Apocrypha") {
        downloads::Begin::Started(d) => d,
        downloads::Begin::AlreadyRunning(d) => return Ok(d),
    };

    let queue = state.downloads.clone();
    let id = entry.id.clone();
    let url = ticket.url.clone();

    // A dedicated thread rather than the async runtime: `ureq` is blocking, and
    // this keeps the transfer off every pool the rest of the app shares.
    std::thread::spawn(move || {
        let emit = {
            let app = app.clone();
            Box::new(move |d: &Download| {
                let _ = app.emit("download-changed", d.clone());
            }) as downloads::OnProgress
        };

        let outcome = downloads::fetch(&queue, &id, &url, &dest, &emit).and_then(|_| {
            // The service publishes the hash it recorded at upload, so there is
            // no reason to install bytes that do not match it. A truncated
            // transfer, a proxy that rewrote something, or a URL that outlived
            // its object all land here rather than in a game directory.
            let actual = apoc_deploy::vault::hash_file(&dest).map_err(|e| e.to_string())?;
            if expected_sha.is_empty() || actual.eq_ignore_ascii_case(&expected_sha) {
                Ok(())
            } else {
                // Removed rather than left: a file in the downloads folder is
                // offered for install, and one that failed its hash must not be
                // sitting there looking like any other archive.
                let _ = std::fs::remove_file(&dest);
                Err(
                    "The download did not match the file the service published. \
                     Nothing was kept."
                        .to_string(),
                )
            }
        });

        if let Err(e) = outcome {
            let cancelled = e == "cancelled";
            if let Some(d) = queue.update(&id, |d| {
                d.state = if cancelled {
                    downloads::DownloadState::Cancelled
                } else {
                    downloads::DownloadState::Failed
                };
                d.error = (!cancelled).then(|| e.clone());
            }) {
                let _ = app.emit("download-changed", d);
            }
        }
    });

    Ok(entry)
}

/// A catalogue client for the signed-in account, or a refusal saying so.
fn catalog(state: &AppState) -> CmdResult<Catalog> {
    let token = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .get_setting(KEY_TOKEN)
            .map_err(|e| e.to_string())?
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| "Sign in to Apocrypha first.".to_string())?
    };
    Ok(Catalog::new(origin(), APP_VERSION, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_name_says_which_computer() {
        // Whoever approves already knows which application is asking.
        let name = default_device_name();
        assert!(!name.is_empty());
        assert!(name.starts_with("Apocrypha"));
    }
}
