//! Getting this installation signed in, without ever seeing a password.
//!
//! The app asks the service to start a pairing and receives two things: a long
//! secret it keeps to itself, and a short code it shows on screen. A person
//! opens the website, checks the short code matches, and approves it. The app
//! polls until the service hands over a token.
//!
//! The short code is not a credential. Someone who reads it off the screen can
//! do nothing with it: approving still requires being signed in to the website
//! and confirming a password there. What it protects against is the app being
//! signed into an account that is not the one sitting in front of it.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{agent, ServiceOrigin};

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("network error: {0}")]
    Http(String),
    #[error("could not read the response: {0}")]
    Decode(String),
    #[error("the service refused: {0}")]
    Refused(String),
    #[error("this pairing expired. Start again.")]
    Expired,
    #[error("the request was declined.")]
    Declined,
    #[error("the service returned an unexpected status {0}")]
    Unexpected(u16),
}

/// What the service hands back when a pairing starts.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedPairing {
    /// The long secret. Never shown, never logged, never left on disk.
    pub device_code: String,
    /// The short code, as the service generated it.
    pub user_code: String,
    /// The same code grouped for reading aloud, which is what to display.
    pub user_code_display: String,
    pub expires_in_seconds: i64,
    pub poll_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantedToken {
    token: String,
    expires_at: String,
}

/// Where a pairing has got to. Returned by one poll, not by a loop: the caller
/// owns the waiting, so a window can stay responsive and a person can cancel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingStatus {
    /// Nobody has decided yet. Poll again after the interval.
    Pending,
    /// Polled too soon. Not an error — wait longer and try again.
    SlowDown,
    /// Approved and collected. This is the only path that produces a token, and
    /// it happens exactly once.
    Granted { token: String, expires_at: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartRequest<'a> {
    device_name: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PollRequest<'a> {
    device_code: &'a str,
}

pub struct DevicePairing {
    origin: ServiceOrigin,
    app_version: String,
}

impl DevicePairing {
    pub fn new(origin: ServiceOrigin, app_version: impl Into<String>) -> Self {
        Self {
            origin,
            app_version: app_version.into(),
        }
    }

    /// Begins a pairing and returns the codes.
    ///
    /// `device_name` is what the approval screen will show, so it should say
    /// something about this machine rather than about this program: the person
    /// approving already knows which app is asking, and needs to know which
    /// computer.
    pub fn start(&self, device_name: &str) -> Result<StartedPairing, PairingError> {
        let url = format!("{}/api/v1/auth/device/start", self.origin.as_str());
        let response = agent()
            .post(&url)
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .send_json(StartRequest { device_name });

        match response {
            Ok(res) => res
                .into_json::<StartedPairing>()
                .map_err(|e| PairingError::Decode(e.to_string())),
            Err(ureq::Error::Status(code, res)) => Err(status_error(code, res)),
            Err(e) => Err(PairingError::Http(e.to_string())),
        }
    }

    /// Asks once whether the pairing has been approved.
    pub fn poll(&self, device_code: &str) -> Result<PairingStatus, PairingError> {
        let url = format!("{}/api/v1/auth/device/token", self.origin.as_str());
        let response = agent()
            .post(&url)
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .send_json(PollRequest { device_code });

        match response {
            Ok(res) => {
                let granted: GrantedToken = res
                    .into_json()
                    .map_err(|e| PairingError::Decode(e.to_string()))?;
                Ok(PairingStatus::Granted {
                    token: granted.token,
                    expires_at: granted.expires_at,
                })
            }
            // Every "not yet" answer arrives as a conflict with a short machine
            // code in the body. Matching on the code rather than the status is
            // what keeps "wait" from being reported to the user as a failure.
            Err(ureq::Error::Status(409, res)) => {
                let body = res.into_string().unwrap_or_default();
                if body.contains("authorization_pending") {
                    Ok(PairingStatus::Pending)
                } else if body.contains("slow_down") {
                    Ok(PairingStatus::SlowDown)
                } else if body.contains("access_denied") {
                    Err(PairingError::Declined)
                } else {
                    Err(PairingError::Refused(safe_message(&body)))
                }
            }
            Err(ureq::Error::Status(404, _)) => Err(PairingError::Expired),
            Err(ureq::Error::Status(code, res)) => Err(status_error(code, res)),
            Err(e) => Err(PairingError::Http(e.to_string())),
        }
    }

    /// The page to open in a browser for this code.
    pub fn approval_url(&self, user_code: &str) -> String {
        self.origin.link_page(user_code)
    }
}

fn status_error(code: u16, res: ureq::Response) -> PairingError {
    let body = res.into_string().unwrap_or_default();
    match code {
        400 | 409 => PairingError::Refused(safe_message(&body)),
        404 => PairingError::Expired,
        _ => PairingError::Unexpected(code),
    }
}

/// Pulls the message out of the service's error shape, and refuses to pass a
/// whole response body through to a user interface if it is not that shape.
///
/// A server that is misbehaving, or something that is not the server at all,
/// should not get to put arbitrary text on screen.
fn safe_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(e) if e.error.len() <= 200 => e.error,
        _ => "The service refused the request.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pending_answer_is_not_an_error() {
        // The three "not yet" answers all arrive as 409 and must be told apart
        // by their code, or waiting looks like failing.
        assert!(matches!(
            safe_message(r#"{"error":"authorization_pending"}"#).as_str(),
            "authorization_pending"
        ));
    }

    #[test]
    fn a_body_that_is_not_the_error_shape_does_not_reach_the_screen() {
        assert_eq!(
            safe_message("<html>gateway timeout</html>"),
            "The service refused the request."
        );
        assert_eq!(safe_message(""), "The service refused the request.");
    }

    #[test]
    fn an_absurdly_long_message_is_replaced_rather_than_shown() {
        let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(500));
        assert_eq!(safe_message(&long), "The service refused the request.");
    }

    #[test]
    fn the_approval_url_is_the_services_own_page() {
        let p = DevicePairing::new(ServiceOrigin::PRODUCTION, "0.5.0");
        let url = p.approval_url("ABCD2345");
        assert!(url.starts_with(ServiceOrigin::PRODUCTION.as_str()));
        assert!(url.contains("ABCD2345"));
    }
}
