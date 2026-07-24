//! Nexus Mods integration: `nxm://` links, the download API, and the Linux
//! protocol handler.
//!
//! # What the API allows
//!
//! This is the constraint the whole design turns on, and it comes straight from
//! the Nexus API documentation:
//!
//! * **Premium accounts** can resolve a download link from a mod id and file id
//!   alone. A manager can fetch unattended.
//! * **Free accounts cannot.** The API refuses with 403 unless the request
//!   carries a `key` and `expires` pair that only the website mints, when the
//!   user clicks "Mod Manager Download". This is deliberate on Nexus's part, so
//!   the correct behaviour for a free account is to open the mod page and wait
//!   for the resulting `nxm://` link rather than to try and work around it.
//!
//! Apocrypha therefore treats a browser round trip as a normal part of the flow
//! rather than an error, and never stores an API key anywhere but the local
//! machine.

pub mod api;
pub mod protocol;
pub mod sso;
pub mod url;

pub use api::{DownloadLink, NexusClient, NexusError, RateLimits, UserInfo};
pub use protocol::{register, status as protocol_status, unregister, Registration};
pub use sso::{sign_in, SsoError, SsoObserver, SsoResult};
pub use url::{parse as parse_nxm, NxmError, NxmLink, NxmTarget};

use serde::{Deserialize, Serialize};

/// Where mod downloads come from. The Apocrypha service is not implemented yet;
/// the setting exists so the switch is in place and the plumbing is not
/// retrofitted later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadSource {
    /// Nexus Mods, via `nxm://` links and the public API.
    Nexus,
    /// The Apocrypha service.
    Apocrypha,
}

impl DownloadSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadSource::Nexus => "nexus",
            DownloadSource::Apocrypha => "apocrypha",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "apocrypha" => DownloadSource::Apocrypha,
            _ => DownloadSource::Nexus,
        }
    }
}

/// The page to open so a free account can mint a download token.
///
/// `nmm=1` is what makes the site show the "Mod Manager Download" button, which
/// is the control that fires the `nxm://` link back at this application.
pub fn mod_page_url(domain: &str, mod_id: u64, file_id: Option<u64>) -> String {
    match file_id {
        Some(f) => format!(
            "https://www.nexusmods.com/{domain}/mods/{mod_id}?tab=files&file_id={f}&nmm=1"
        ),
        None => format!("https://www.nexusmods.com/{domain}/mods/{mod_id}?tab=files&nmm=1"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_page_url_requests_the_manager_download_button() {
        let u = mod_page_url("monsterhunterwilds", 93, Some(2412));
        assert!(u.starts_with("https://www.nexusmods.com/monsterhunterwilds/mods/93"));
        assert!(
            u.contains("nmm=1"),
            "without nmm=1 the site does not offer a manager download"
        );
        assert!(u.contains("file_id=2412"));
    }

    #[test]
    fn download_source_round_trips() {
        assert_eq!(DownloadSource::parse("apocrypha"), DownloadSource::Apocrypha);
        assert_eq!(DownloadSource::parse("nexus"), DownloadSource::Nexus);
        assert_eq!(DownloadSource::parse("nonsense"), DownloadSource::Nexus);
    }
}
