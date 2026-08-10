//! Signing this installation in to the Apocrypha service.
//!
//! Three commands and no loop: the interface starts an authorization, opens the
//! browser, and polls on its own timer. Waiting belongs to the window, which
//! can stay responsive and let someone cancel; a blocking call here would take
//! the choice away and hold a worker thread for five minutes.
//!
//! What is being polled is not the service. It is a socket this process opened
//! on the loopback interface, which is where the browser delivers the answer —
//! see `apoc_apocrypha::oauth` for why the answer must arrive there rather than
//! be collected by whoever started the request.

use apoc_apocrypha::{
    protocol, AuthorizationStatus, Catalog, CatalogGame, CatalogModDetail, CatalogPage,
    DownloadQuota, PendingAuthorization, ServiceOrigin,
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

/// What the window needs in order to wait.
///
/// Be precise about what crosses this boundary. The **verifier** does not: it
/// stays in the pending authorization on this side, and it is the thing that
/// spends the code. Neither does the code, which goes from the browser to the
/// loopback socket and no further.
///
/// The `state` does cross, inside the URL, because it has to — it travels in
/// the browser's address bar by design. It is a correlation value rather than a
/// credential: knowing it lets something claim to be the browser coming back,
/// which gets it as far as a code it cannot exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStartedView {
    pub authorize_url: String,
    pub expires_in_seconds: i64,
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationPollView {
    /// `waiting`, `granted`, or `declined`.
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

/// Opens the callback socket and returns the page to send the browser to.
///
/// The pending authorization — the socket, the verifier, the state — stays in
/// application state rather than going to the interface. There is nothing the
/// window could usefully do with it and every reason not to hand a credential
/// through an IPC boundary.
///
/// Starting a second one abandons the first. That is what pressing Sign in
/// again means, and leaving the old socket open would be a listening port
/// nobody is waiting on.
#[tauri::command(async)]
pub fn start_apocrypha_authorization(
    state: State<AppState>,
) -> CmdResult<AuthorizationStartedView> {
    let name = default_device_name();
    let pending =
        PendingAuthorization::begin(origin(), APP_VERSION, &name).map_err(|e| e.to_string())?;

    let view = AuthorizationStartedView {
        authorize_url: pending.authorize_url().to_string(),
        expires_in_seconds: AUTHORIZATION_WINDOW_SECONDS,
        poll_interval_seconds: POLL_INTERVAL_SECONDS,
    };

    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(KEY_DEVICE_NAME, &name)
            .map_err(|e| e.to_string())?;
    }

    *state
        .pending_authorization
        .lock()
        .map_err(|_| "state poisoned")? = Some(pending);

    Ok(view)
}

/// How long the window keeps asking, matching the socket's own deadline.
const AUTHORIZATION_WINDOW_SECONDS: i64 = 300;

/// How often the window asks. Nothing is being requested of the service here —
/// this is a non-blocking accept on a local socket — so the interval is about
/// how quickly the app should notice, not about being polite to a server.
const POLL_INTERVAL_SECONDS: u64 = 1;

/// One check. Stores the token itself when it arrives, so the secret never
/// reaches the interface.
#[tauri::command(async)]
pub fn poll_apocrypha_authorization(state: State<AppState>) -> CmdResult<AuthorizationPollView> {
    let mut slot = state
        .pending_authorization
        .lock()
        .map_err(|_| "state poisoned")?;
    let pending = slot
        .as_ref()
        .ok_or_else(|| "No sign-in is in progress.".to_string())?;

    match pending.poll() {
        Ok(AuthorizationStatus::Waiting) => Ok(AuthorizationPollView {
            status: "waiting".into(),
        }),
        Ok(AuthorizationStatus::Declined) => {
            *slot = None;
            Ok(AuthorizationPollView {
                status: "declined".into(),
            })
        }
        Ok(AuthorizationStatus::Granted { token, expires_at }) => {
            // Closed before the token is stored, not after. The socket has done
            // its job and leaving it open is a port accepting connections for a
            // sign-in that already finished.
            *slot = None;
            drop(slot);

            let store = state.store.lock().map_err(|_| "state poisoned")?;
            store
                .set_setting(KEY_TOKEN, &token)
                .map_err(|e| e.to_string())?;
            store
                .set_setting(KEY_TOKEN_EXPIRES, &expires_at)
                .map_err(|e| e.to_string())?;
            Ok(AuthorizationPollView {
                status: "granted".into(),
            })
        }
        Err(e) => {
            // Any error ends the attempt. The socket is not reusable after one
            // — an expired window stays expired, and an impostor on the port is
            // a reason to stop rather than to keep listening.
            *slot = None;
            Err(e.to_string())
        }
    }
}

/// Abandons an authorization in progress, closing the socket.
#[tauri::command(async)]
pub fn cancel_apocrypha_authorization(state: State<AppState>) -> CmdResult<()> {
    *state
        .pending_authorization
        .lock()
        .map_err(|_| "state poisoned")? = None;
    Ok(())
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
    let result = catalog(&state)?.mods(game.as_deref(), search.as_deref(), page, 24);
    with_token_check(&state, result)
}

/// Every game the service lists.
///
/// The window uses this to decide whether the game it is managing exists on the
/// service at all, which is the difference between "no mods published yet" and
/// "not listed here" — two states that need different words.
#[tauri::command(async)]
pub fn apocrypha_games(state: State<AppState>) -> CmdResult<Vec<CatalogGame>> {
    let result = catalog(&state)?.games();
    with_token_check(&state, result)
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
    let result = catalog(&state)?.mod_detail(&game_slug, &mod_slug);
    with_token_check(&state, result)
}

/// What is left of today's download allowance.
#[tauri::command(async)]
pub fn apocrypha_download_quota(state: State<AppState>) -> CmdResult<DownloadQuota> {
    let result = catalog(&state)?.download_quota();
    with_token_check(&state, result)
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

    let dest_dir = state.downloads_dir();
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;

    // The mod slug is part of the name on disk, because the queue identifies a
    // transfer by where it is writing and file names are not unique across the
    // catalogue. Two mods both shipping "main.zip" would otherwise be one
    // download: the second would be handed the first one back as already
    // running, and reported as queued while none of its bytes ever were.
    //
    // Taken from the catalogue rather than from the ticket, so the name is
    // known before anything is claimed.
    let dest = dest_dir.join(downloads::safe_name(&format!(
        "{mod_slug}-{}",
        file.file_name
    )));

    // Reserved before the claim, not after. Claiming spends a slot against the
    // daily allowance, and a second press on a download already running must
    // not spend one to be told it was already running.
    let entry = match state.downloads.begin(&file.file_name, &dest, "Apocrypha") {
        downloads::Begin::Started(d) => d,
        downloads::Begin::AlreadyRunning(d) => return Ok(d),
    };

    let ticket = match catalog.claim_download(&file_id) {
        Ok(t) => t,
        Err(e) => {
            // The slot was reserved on the assumption this would work. Release
            // it, or the name stays occupied and every later attempt is told it
            // is already downloading.
            state.downloads.forget(&entry.id);
            return Err(e.to_string());
        }
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

/// What an `apocrypha://` link turns out to refer to, once the service has been
/// asked.
///
/// Every field here comes from the service. Nothing a link said is shown back to
/// anyone: a name or a size taken from the link would be a stranger's words
/// presented as the platform's, which is the whole trick a confirmation screen
/// has to not fall for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreviewView {
    pub game_slug: String,
    pub mod_slug: String,
    pub file_id: String,
    pub game_name: String,
    pub mod_name: String,
    pub author_name: String,
    pub version: Option<String>,
    pub file_label: String,
    pub size_bytes: i64,
    /// Whether the service will serve these bytes: scanned clean, bytes verified.
    pub ready: bool,
    /// What is left of today's allowance, when there is a limit at all.
    pub remaining_today: Option<i32>,
}

/// Resolve a link into something a person can agree to.
///
/// Deliberately read-only. It reads the mod and the allowance and claims
/// nothing, so a page that fires this repeatedly cannot spend anything — the
/// quota slot is spent by the download command, after someone has said yes.
///
/// Refuses outright when this computer is not paired, rather than remembering
/// the link for after a sign-in. A link kept across an authentication turns a
/// page anyone can publish into an action taken later, when whoever clicked has
/// forgotten why the app is asking.
#[tauri::command(async)]
pub fn preview_apocrypha_link(state: State<AppState>, url: String) -> CmdResult<LinkPreviewView> {
    let request = protocol::parse(&url).map_err(|e| e.message().to_string())?;

    // Same refusal as everywhere else, and for the same reason: the catalogue is
    // read as the account, so there has to be one.
    let catalog = catalog(&state)?;

    let detail = catalog
        .mod_detail(&request.game_slug, &request.mod_slug)
        .map_err(|e| e.to_string())?;

    // The link named a game, a mod and a file independently, so nothing so far
    // has established that the file belongs to the other two. Without this a
    // link could pair one mod's name with another mod's file and have the
    // confirmation describe something other than what would be fetched.
    let (version, file) = detail
        .versions
        .iter()
        .find_map(|v| {
            v.files
                .iter()
                .find(|f| f.id == request.file_id)
                .map(|f| (v, f))
        })
        .ok_or_else(|| "That file is not part of this mod.".to_string())?;

    // Best effort: an allowance that will not load should not stop someone
    // seeing what the link points at.
    let remaining_today = catalog.download_quota().ok().and_then(|q| q.remaining);

    Ok(LinkPreviewView {
        game_slug: request.game_slug,
        mod_slug: request.mod_slug,
        file_id: request.file_id,
        game_name: detail.game_name.clone(),
        mod_name: detail.name.clone(),
        author_name: detail.author_name.clone(),
        version: Some(version.version_number.clone()).filter(|v| !v.is_empty()),
        file_label: file.label().to_string(),
        size_bytes: file.size_bytes,
        ready: file.is_downloadable(),
        remaining_today,
    })
}

/// Forgets the stored token, without asking the service anything.
///
/// Called when the service says the token is no longer good — because the
/// person revoked this device from the website, or it expired. Keeping a dead
/// credential on disk means the Account screen says "signed in" while every
/// request fails, which reads as the app being broken rather than as the
/// deliberate act it was.
fn forget_token(state: &AppState) {
    if let Ok(store) = state.store.lock() {
        let _ = store.set_setting(KEY_TOKEN, "");
        let _ = store.set_setting(KEY_TOKEN_EXPIRES, "");
    }
}

/// Runs a catalogue call, and clears the stored token if the service says it is
/// no longer valid.
///
/// Only on an explicit refusal of the credential. A network failure must never
/// sign someone out — the token is probably fine and the connection is not, and
/// silently discarding a ninety-day grant because a redeploy dropped one
/// request would be its own bug.
fn with_token_check<T>(
    state: &AppState,
    result: Result<T, apoc_apocrypha::AuthorizationError>,
) -> CmdResult<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            if matches!(&e, apoc_apocrypha::AuthorizationError::Refused(m) if m.contains("not signed in"))
            {
                forget_token(state);
            }
            Err(e.to_string())
        }
    }
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
