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

pub use catalog::{
    Catalog, CatalogFile, CatalogMod, CatalogModDetail, CatalogPage, CatalogVersion, DownloadQuota,
    DownloadTicket,
};
pub use pairing::{DevicePairing, PairingError, PairingStatus, StartedPairing};

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

#[cfg(test)]
mod tests {
    use super::*;

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
