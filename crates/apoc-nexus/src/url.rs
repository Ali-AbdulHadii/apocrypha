//! Parser for `nxm://` links.
//!
//! The website's "Mod Manager Download" button fires a link at whichever
//! application is registered for the scheme. The shape is:
//!
//! ```text
//! nxm://<game-domain>/mods/<modId>/files/<fileId>?key=..&expires=..&user_id=..
//! nxm://<game-domain>/collections/<slug>/revisions/<n|latest>?...
//! ```
//!
//! For a free account the `key` and `expires` parameters are mandatory and
//! short lived: they are what proves the user visited the site. Unknown query
//! parameters must be tolerated, because Nexus adds them over time.

use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NxmError {
    #[error("not an nxm:// link")]
    WrongScheme,
    #[error("nxm link is missing a game domain")]
    MissingDomain,
    #[error("unrecognised nxm link path: {0}")]
    UnsupportedPath(String),
    #[error("nxm link has a non-numeric id: {0}")]
    BadId(String),
}

/// What an `nxm://` link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NxmTarget {
    Mod { mod_id: u64, file_id: u64 },
    Collection { slug: String, revision: String },
}

/// A parsed `nxm://` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NxmLink {
    /// Nexus game domain, e.g. `monsterhunterwilds`. Also the API path segment.
    pub domain: String,
    pub target: NxmTarget,
    /// One-shot download token. Absent on links minted for premium accounts.
    pub key: Option<String>,
    /// Unix seconds at which `key` stops working.
    pub expires: Option<u64>,
    /// The Nexus account the link was minted for.
    pub user_id: Option<u64>,
    /// `view=true` means focus the mod in the manager rather than download it.
    pub view_only: bool,
    /// Every other query parameter, kept rather than dropped.
    pub extra: BTreeMap<String, String>,
}

impl NxmLink {
    pub fn mod_ids(&self) -> Option<(u64, u64)> {
        match self.target {
            NxmTarget::Mod { mod_id, file_id } => Some((mod_id, file_id)),
            _ => None,
        }
    }

    /// True when the link carries the site-issued token a free account needs.
    pub fn has_token(&self) -> bool {
        self.key.is_some() && self.expires.is_some()
    }

    /// Whether the token is still within its validity window.
    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.expires.is_some_and(|e| now_unix >= e)
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse an `nxm://` link. Unknown query parameters are preserved, not rejected.
pub fn parse(raw: &str) -> Result<NxmLink, NxmError> {
    let rest = raw
        .strip_prefix("nxm://")
        .or_else(|| raw.strip_prefix("NXM://"))
        .ok_or(NxmError::WrongScheme)?;

    let (before_query, query) = match rest.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };

    let mut segments = before_query.split('/').filter(|s| !s.is_empty());
    let domain = segments.next().ok_or(NxmError::MissingDomain)?.to_string();
    let parts: Vec<&str> = segments.collect();

    let parse_id = |s: &str| s.parse::<u64>().map_err(|_| NxmError::BadId(s.to_string()));

    let target = match parts.as_slice() {
        ["mods", m, "files", f] => NxmTarget::Mod {
            mod_id: parse_id(m)?,
            file_id: parse_id(f)?,
        },
        ["collections", slug, "revisions", rev] => NxmTarget::Collection {
            slug: (*slug).to_string(),
            revision: (*rev).to_string(),
        },
        _ => return Err(NxmError::UnsupportedPath(before_query.to_string())),
    };

    let mut key = None;
    let mut expires = None;
    let mut user_id = None;
    let mut view_only = false;
    let mut extra = BTreeMap::new();

    if let Some(q) = query {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = percent_decode(v);
            match k {
                "key" => key = Some(v),
                "expires" => expires = v.parse().ok(),
                "user_id" => user_id = v.parse().ok(),
                "view" => view_only = v == "true" || v == "1",
                other => {
                    extra.insert(other.to_string(), v);
                }
            }
        }
    }

    Ok(NxmLink {
        domain,
        target,
        key,
        expires,
        user_id,
        view_only,
        extra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_free_account_mod_link() {
        let l = parse(
            "nxm://monsterhunterwilds/mods/93/files/2412\
             ?key=k3Yb1TzQ9pLmWn4RxVaS7Q&expires=1785062400&user_id=81948013",
        )
        .unwrap();

        assert_eq!(l.domain, "monsterhunterwilds");
        assert_eq!(l.mod_ids(), Some((93, 2412)));
        assert_eq!(l.key.as_deref(), Some("k3Yb1TzQ9pLmWn4RxVaS7Q"));
        assert_eq!(l.expires, Some(1785062400));
        assert_eq!(l.user_id, Some(81948013));
        assert!(l.has_token());
        assert!(!l.view_only);
    }

    #[test]
    fn parses_a_premium_link_with_no_token() {
        let l = parse("nxm://monsterhunterwilds/mods/93/files/2412").unwrap();
        assert_eq!(l.mod_ids(), Some((93, 2412)));
        assert!(!l.has_token(), "premium links carry no key");
    }

    #[test]
    fn parses_a_collection_link() {
        let l = parse("nxm://monsterhunterwilds/collections/abc123/revisions/latest").unwrap();
        assert_eq!(
            l.target,
            NxmTarget::Collection {
                slug: "abc123".into(),
                revision: "latest".into(),
            }
        );
    }

    #[test]
    fn unknown_query_parameters_are_kept_not_rejected() {
        // Nexus adds parameters over time; dropping the link would break downloads.
        let l = parse("nxm://game/mods/1/files/2?key=k&expires=9&something_new=42").unwrap();
        assert_eq!(l.extra.get("something_new").map(String::as_str), Some("42"));
    }

    #[test]
    fn view_links_are_flagged() {
        let l = parse("nxm://game/mods/1/files/2?view=true").unwrap();
        assert!(l.view_only, "a view link must not start a download");
    }

    #[test]
    fn rejects_malformed_links() {
        assert_eq!(parse("https://example.com").unwrap_err(), NxmError::WrongScheme);
        assert!(matches!(
            parse("nxm://game/mods/abc/files/2"),
            Err(NxmError::BadId(_))
        ));
        assert!(matches!(
            parse("nxm://game/mods/1"),
            Err(NxmError::UnsupportedPath(_))
        ));
        assert!(matches!(
            parse("nxm://game/something/1/else/2"),
            Err(NxmError::UnsupportedPath(_))
        ));
    }

    #[test]
    fn expiry_is_evaluated_against_the_clock() {
        let l = parse("nxm://g/mods/1/files/2?key=k&expires=1000").unwrap();
        assert!(!l.is_expired(999));
        assert!(l.is_expired(1000));
        assert!(l.is_expired(5000));
    }

    #[test]
    fn percent_encoded_values_are_decoded() {
        let l = parse("nxm://g/mods/1/files/2?key=a%2Bb%2Fc").unwrap();
        assert_eq!(l.key.as_deref(), Some("a+b/c"));
    }
}
