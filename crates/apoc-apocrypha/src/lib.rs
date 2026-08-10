//! Client for the Apocrypha service.
//!
//! First job: getting signed in. The app never asks for a password. It asks the
//! service to start a pairing, shows the short code it gets back, opens the
//! website, and waits for a person to approve it there. What comes back is a
//! scoped token, which the caller stores. Today that is the same settings table
//! the Nexus API key uses; moving both to the OS keyring is one change for both
//! and is not this one.
//!
//! # The origin is compiled in, not configured by a link
//!
//! [`ServiceOrigin`] is the one thing here that must never come from anywhere
//! but this binary. Everything the client will eventually do — resolving an
//! `apocrypha://` link, fetching a download, trusting a hash — is only as
//! trustworthy as the host it asked. A configurable origin turns a crafted link
//! or a tampered settings file into "download from wherever I say", which is
//! the whole attack. The one exception is a developer build pointing at
//! localhost, and that is a compile-time feature rather than a runtime setting.

pub mod catalog;
pub mod pairing;
pub mod protocol;

pub use catalog::{
    Catalog, CatalogFile, CatalogGame, CatalogMod, CatalogModDetail, CatalogPage,
    CatalogRelationship, CatalogVersion, DownloadQuota, DownloadTicket,
};
pub use pairing::{DevicePairing, PairingError, PairingStatus, StartedPairing};
pub use protocol::{InstallRequest, LinkError};

use std::time::Duration;

/// Where the service lives. Constructed only from the constants below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceOrigin(&'static str);

impl ServiceOrigin {
    /// The real service.
    pub const PRODUCTION: ServiceOrigin = ServiceOrigin("https://apocryphamods.com");

    /// A local API, for development. Behind a feature so a release build cannot
    /// be pointed at plain HTTP on localhost by accident or by a settings file.
    #[cfg(feature = "local-service")]
    pub const LOCAL: ServiceOrigin = ServiceOrigin("http://localhost:5099");

    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// The page a browser is sent to so a person can approve a pairing.
    pub fn link_page(&self, user_code: &str) -> String {
        format!("{}/link?code={}", self.0, urlencode(user_code))
    }
}

impl Default for ServiceOrigin {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

/// Percent-encodes the few characters a user code could contain that would
/// change a URL's meaning.
///
/// The alphabet is already restricted to unreserved characters, so this encodes
/// nothing in practice — it exists so that a change to the alphabet upstream
/// cannot quietly turn a code into a query parameter.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            for b in c.to_string().bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .build()
}

/// Attempts a request makes before giving up.
///
/// Two, not more. The service redeploys on every push to its main branch and
/// recreates its containers in place, so a dropped connection lasting a second
/// or two is a routine event rather than an exceptional one — and a single
/// retry covers it. Beyond that, retrying stops being resilience and starts
/// being a client that will not take no for an answer.
const ATTEMPTS: u32 = 2;

/// How long to wait before the second attempt.
const RETRY_DELAY: Duration = Duration::from_millis(750);

/// Runs a request, retrying once if the connection failed.
///
/// **Only transport failures are retried.** A status is an answer: a 403 means
/// the same thing however many times it is asked, and repeating the question
/// only delays telling the person what happened. `ureq` draws exactly this
/// line — `Error::Status` is a reply, `Error::Transport` is the absence of one.
///
/// A transport failure can still occur after the server has acted, when the
/// reply is what got lost. Retrying then repeats the effect. That is accepted
/// here because every request this client makes is one where repeating it is
/// no worse than the person clicking the button again, which is exactly what
/// they would otherwise do: a second pairing row nobody collects expires on
/// its own, and a second download claim for a mod already claimed today costs
/// nothing, because the daily allowance counts distinct mods rather than
/// requests. A request without that property must not use this.
// `ureq::Error` is 272 bytes, which clippy objects to carrying in a Result.
// Boxing it is the suggested fix and is the wrong trade here: every caller
// matches on `Error::Status(code, response)` to read the service's own refusal,
// and each would have to unbox first — real friction at four call sites, to
// satisfy a lint about the size of a type this crate does not define.
#[allow(clippy::result_large_err)]
pub(crate) fn send_retrying<F>(send: F) -> Result<ureq::Response, ureq::Error>
where
    F: FnMut() -> Result<ureq::Response, ureq::Error>,
{
    retrying(send, |e| matches!(e, ureq::Error::Transport(_)))
}

/// The retry loop itself, with the decision handed in.
///
/// Split from [`send_retrying`] so the behaviour that matters — how many times,
/// and never on an answer — can be tested. `ureq::Transport` has no public
/// constructor, so a test written against the real type could only exercise the
/// path that does not retry, which is the half that was never in doubt.
pub(crate) fn retrying<T, E, F, P>(mut send: F, is_transient: P) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
    P: Fn(&E) -> bool,
{
    let mut attempt = 1;
    loop {
        match send() {
            Err(e) if attempt < ATTEMPTS && is_transient(&e) => {
                // The detail is not logged. It is a rustls or io message that
                // means nothing to the person waiting, and the next attempt is
                // about to say whether it mattered at all.
                std::thread::sleep(RETRY_DELAY);
                attempt += 1;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for a failure, since ureq's own transport error cannot be
    /// constructed from outside the crate.
    #[derive(Debug, PartialEq)]
    enum Fail {
        Dropped,
        Answered,
    }
    fn transient(e: &Fail) -> bool {
        *e == Fail::Dropped
    }

    #[test]
    fn a_dropped_connection_is_tried_once_more() {
        // The case this exists for: the service redeploys, one connection is
        // dropped, the next succeeds. Without the retry that is a failed
        // sign-in and an alarming message; with it, nothing happened.
        let mut calls = 0;
        let result = retrying(
            || {
                calls += 1;
                if calls == 1 {
                    Err(Fail::Dropped)
                } else {
                    Ok("served")
                }
            },
            transient,
        );
        assert_eq!(result, Ok("served"));
        assert_eq!(calls, 2, "should have tried again exactly once");
    }

    #[test]
    fn an_answer_is_never_repeated() {
        // A refusal means the same thing however many times it is asked.
        // Repeating it only delays saying so, and on a write it would repeat
        // whatever the server already did.
        let mut calls = 0;
        let result: Result<&str, Fail> = retrying(
            || {
                calls += 1;
                Err(Fail::Answered)
            },
            transient,
        );
        assert_eq!(result, Err(Fail::Answered));
        assert_eq!(calls, 1, "an answer must not be retried");
    }

    #[test]
    fn retrying_is_bounded_rather_than_persistent() {
        let mut calls = 0;
        let result: Result<&str, Fail> = retrying(
            || {
                calls += 1;
                Err(Fail::Dropped)
            },
            transient,
        );
        assert!(result.is_err());
        assert_eq!(calls, ATTEMPTS, "gives up rather than hammering");
    }

    #[test]
    fn the_link_page_carries_the_code() {
        let url = ServiceOrigin::PRODUCTION.link_page("ABCD2345");
        assert!(url.starts_with("https://apocryphamods.com/link?code="));
        assert!(url.ends_with("ABCD2345"));
    }

    #[test]
    fn the_production_origin_is_https() {
        // A plain-HTTP origin would make every later guarantee — the hash the
        // API reports, the download location it names — worthless.
        assert!(ServiceOrigin::PRODUCTION.as_str().starts_with("https://"));
    }

    #[test]
    fn anything_that_could_change_a_url_is_encoded() {
        assert_eq!(urlencode("ABCD2345"), "ABCD2345");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("../x"), "..%2Fx");
    }
}
