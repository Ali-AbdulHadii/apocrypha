//! Client for the Nexus Mods public API (v1, the only version that issues
//! download links).
//!
//! Quota is driven by the `X-RL-*` response headers rather than hardcoded
//! numbers, because the published limits and the documented limits disagree and
//! have changed over time. Reading them from the response is the only reliable
//! way to know where we stand.

use serde::Deserialize;
use std::time::Duration;
use thiserror::Error;

const API_BASE: &str = "https://api.nexusmods.com/v1";

#[derive(Debug, Error)]
pub enum NexusError {
    #[error("network error: {0}")]
    Http(String),
    #[error("could not read the response: {0}")]
    Decode(String),
    #[error("the API key was rejected. Check it in Settings.")]
    Unauthorized,
    #[error(
        "this account cannot request downloads directly. Open the mod page and use \
         the Mod Manager Download button."
    )]
    PremiumRequired,
    #[error("the download token is not valid for this account")]
    WrongAccount,
    #[error("the download link has expired. Open the mod page again.")]
    Expired,
    #[error("file not found on Nexus Mods")]
    NotFound,
    #[error("Nexus Mods rate limit reached. Try again after {0}.")]
    RateLimited(String),
    #[error("Nexus Mods returned an unexpected status {0}")]
    Unexpected(u16),
}

/// Quota state, read from the response headers on every call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateLimits {
    pub hourly_limit: Option<u32>,
    pub hourly_remaining: Option<u32>,
    pub hourly_reset: Option<String>,
    pub daily_limit: Option<u32>,
    pub daily_remaining: Option<u32>,
    pub daily_reset: Option<String>,
}

impl RateLimits {
    /// True when the next request would certainly be refused.
    pub fn exhausted(&self) -> bool {
        self.hourly_remaining == Some(0) || self.daily_remaining == Some(0)
    }
}

/// The account behind an API key.
#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    pub user_id: u64,
    pub name: String,
    #[serde(default)]
    pub is_premium: bool,
    #[serde(default)]
    pub is_supporter: bool,
}

/// One CDN choice returned by the download-link endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct DownloadLink {
    #[serde(rename = "URI")]
    pub uri: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub short_name: String,
}

/// Metadata for one uploaded file.
#[derive(Debug, Clone, Deserialize)]
pub struct FileInfo {
    pub file_id: u64,
    pub name: String,
    pub file_name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Nexus reports this in kilobytes, not bytes.
    #[serde(default)]
    pub size: u64,
}

/// One entry in a mod's file listing.
///
/// Distinct from [`FileInfo`], which is the single-file endpoint: that one is
/// asked about a file you already know, this one is what you get when asking
/// what a mod has. `category_name` is optional because Nexus reports `null` for
/// archived files rather than omitting them.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModFile {
    pub file_id: u64,
    pub name: String,
    pub file_name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub category_name: Option<String>,
    #[serde(default)]
    pub uploaded_timestamp: i64,
    /// Nexus reports this in kilobytes, not bytes.
    #[serde(default)]
    pub size: u64,
}

/// An author's declaration that one file replaces another.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FileUpdate {
    pub old_file_id: u64,
    pub new_file_id: u64,
    #[serde(default)]
    pub uploaded_timestamp: i64,
}

/// Everything the files endpoint returns for one mod.
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ModFiles {
    #[serde(default)]
    pub files: Vec<ModFile>,
    #[serde(default)]
    pub file_updates: Vec<FileUpdate>,
}

pub struct NexusClient {
    api_key: String,
    app_name: String,
    app_version: String,
    agent: ureq::Agent,
}

fn header_u32(res: &ureq::Response, name: &str) -> Option<u32> {
    res.header(name).and_then(|v| v.trim().parse().ok())
}

fn header_string(res: &ureq::Response, name: &str) -> Option<String> {
    res.header(name).map(|v| v.trim().to_string())
}

fn limits_from(res: &ureq::Response) -> RateLimits {
    RateLimits {
        hourly_limit: header_u32(res, "X-RL-Hourly-Limit"),
        hourly_remaining: header_u32(res, "X-RL-Hourly-Remaining"),
        hourly_reset: header_string(res, "X-RL-Hourly-Reset"),
        daily_limit: header_u32(res, "X-RL-Daily-Limit"),
        daily_remaining: header_u32(res, "X-RL-Daily-Remaining"),
        daily_reset: header_string(res, "X-RL-Daily-Reset"),
    }
}

impl NexusClient {
    pub fn new(api_key: impl Into<String>, app_name: &str, app_version: &str) -> Self {
        NexusClient {
            api_key: api_key.into(),
            app_name: app_name.to_string(),
            app_version: app_version.to_string(),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(30))
                .build(),
        }
    }

    fn get(&self, url: &str) -> Result<(ureq::Response, RateLimits), NexusError> {
        // Identifying the application honestly is required by the API terms.
        let req = self
            .agent
            .get(url)
            .set("apikey", &self.api_key)
            .set("Application-Name", &self.app_name)
            .set("Application-Version", &self.app_version)
            .set("Accept", "application/json");

        match req.call() {
            Ok(res) => {
                let limits = limits_from(&res);
                Ok((res, limits))
            }
            Err(ureq::Error::Status(code, res)) => {
                let limits = limits_from(&res);
                Err(match code {
                    400 => NexusError::WrongAccount,
                    401 => NexusError::Unauthorized,
                    403 => NexusError::PremiumRequired,
                    404 => NexusError::NotFound,
                    410 => NexusError::Expired,
                    429 => NexusError::RateLimited(
                        limits
                            .hourly_reset
                            .clone()
                            .or(limits.daily_reset.clone())
                            .unwrap_or_else(|| "a while".into()),
                    ),
                    other => NexusError::Unexpected(other),
                })
            }
            Err(e) => Err(NexusError::Http(e.to_string())),
        }
    }

    /// Check the key and learn whether the account is premium.
    ///
    /// This endpoint does not count against the hourly quota.
    pub fn validate(&self) -> Result<(UserInfo, RateLimits), NexusError> {
        let (res, limits) = self.get(&format!("{API_BASE}/users/validate.json"))?;
        let user: UserInfo = res
            .into_json()
            .map_err(|e| NexusError::Decode(e.to_string()))?;
        Ok((user, limits))
    }

    /// Resolve the actual download URLs for a file.
    ///
    /// `token` is the `(key, expires)` pair from an `nxm://` link. A free
    /// account must supply it; a premium account may omit it.
    pub fn download_link(
        &self,
        domain: &str,
        mod_id: u64,
        file_id: u64,
        token: Option<(&str, u64)>,
    ) -> Result<(Vec<DownloadLink>, RateLimits), NexusError> {
        let mut url =
            format!("{API_BASE}/games/{domain}/mods/{mod_id}/files/{file_id}/download_link.json");
        if let Some((key, expires)) = token {
            url.push_str(&format!("?key={key}&expires={expires}"));
        }
        let (res, limits) = self.get(&url)?;
        let links: Vec<DownloadLink> = res
            .into_json()
            .map_err(|e| NexusError::Decode(e.to_string()))?;
        Ok((links, limits))
    }

    /// Metadata for one file, used to name the download and show its version.
    pub fn file_info(
        &self,
        domain: &str,
        mod_id: u64,
        file_id: u64,
    ) -> Result<(FileInfo, RateLimits), NexusError> {
        let (res, limits) = self.get(&format!(
            "{API_BASE}/games/{domain}/mods/{mod_id}/files/{file_id}.json"
        ))?;
        let info: FileInfo = res
            .into_json()
            .map_err(|e| NexusError::Decode(e.to_string()))?;
        Ok((info, limits))
    }

    /// Every file a mod has, plus the author's replacement links.
    ///
    /// One request per mod, which is what makes an update check expensive: a
    /// library of two hundred linked mods is two hundred requests against an
    /// hourly quota. Callers are expected to watch [`RateLimits::exhausted`]
    /// and stop, rather than discovering the limit by being refused.
    pub fn mod_files(
        &self,
        domain: &str,
        mod_id: u64,
    ) -> Result<(ModFiles, RateLimits), NexusError> {
        let (res, limits) = self.get(&format!(
            "{API_BASE}/games/{domain}/mods/{mod_id}/files.json"
        ))?;
        let files: ModFiles = res
            .into_json()
            .map_err(|e| NexusError::Decode(e.to_string()))?;
        Ok((files, limits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_is_detected_from_either_window() {
        let mut l = RateLimits::default();
        assert!(!l.exhausted(), "unknown quota is not exhausted quota");
        l.hourly_remaining = Some(0);
        assert!(l.exhausted());
        l.hourly_remaining = Some(5);
        l.daily_remaining = Some(0);
        assert!(l.exhausted());
    }

    #[test]
    fn errors_explain_what_the_user_should_do() {
        // A free account hitting the premium-only path is the single most common
        // failure, so its message has to say what to do next.
        let msg = NexusError::PremiumRequired.to_string();
        assert!(msg.contains("Mod Manager Download"));
        assert!(NexusError::Expired.to_string().contains("mod page"));
        assert!(NexusError::Unauthorized.to_string().contains("Settings"));
    }
}
