//! Signing this installation in to the Apocrypha service.
//!
//! Three commands and no loop: the interface starts a pairing, opens the
//! browser, and polls on its own timer. Waiting belongs to the window, which
//! can stay responsive and let someone cancel; a blocking call here would take
//! the choice away and hold a worker thread for ten minutes.

use apoc_apocrypha::{DevicePairing, PairingStatus, ServiceOrigin};
use serde::{Deserialize, Serialize};
use tauri::State;

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
