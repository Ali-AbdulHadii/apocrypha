//! Noticing that a newer Apocrypha has been released.
//!
//! This only ever *tells* you. It does not download, replace or run anything.
//! Self-replacement needs signed artifacts and a release pipeline that signs
//! them; until that exists, a key would live on one machine and losing it would
//! strand every installation. Saying "0.5.0 is out, here is the page" carries
//! none of that risk and most of the value, which is that people find out at
//! all.
//!
//! How the app was installed decides what the advice should be, so it is
//! detected rather than guessed: replacing a file is right for an AppImage and
//! wrong for a package the system owns.

use serde::{Deserialize, Serialize};

const RELEASES_API: &str = "https://api.github.com/repos/Apocrypha-Mods/apocrypha/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/Apocrypha-Mods/apocrypha/releases/latest";

/// How this copy of Apocrypha got onto the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    /// A single file the user can replace themselves.
    AppImage,
    /// Installed by the system package manager, which owns the files. Telling
    /// someone to overwrite these would fight their package manager and lose.
    Package,
    /// A development build. Never worth nagging.
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateView {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub url: String,
    pub install_kind: InstallKind,
}

/// A release version, compared the way semantic versioning says to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    parts: (u64, u64, u64),
    /// `Some` for a pre-release like `0.5.0-beta.1`, which ranks *below* the
    /// release it leads to.
    pre: Option<String>,
}

fn parse(v: &str) -> Option<Version> {
    let v = v.trim();
    // Tags are published as `v0.4.0`; Cargo reports `0.4.0`. Accept both so the
    // comparison never depends on which side it came from.
    let v = v.strip_prefix('v').unwrap_or(v);
    // Build metadata is not part of precedence, so it is discarded.
    let v = v.split('+').next().unwrap_or(v);

    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (v, None),
    };

    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    // A tag of `0.5` is a real thing people publish; treat the absent parts as
    // zero rather than refusing to compare.
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;

    Some(Version {
        parts: (major, minor, patch),
        pre,
    })
}

/// Whether `latest` is a release worth telling the user about.
///
/// Unparseable input on either side answers `false`. A version we cannot
/// understand is not grounds for claiming an update exists.
pub fn is_newer(current: &str, latest: &str) -> bool {
    let (Some(c), Some(l)) = (parse(current), parse(latest)) else {
        return false;
    };

    if l.parts != c.parts {
        return l.parts > c.parts;
    }

    // Same numbers: a release beats the pre-release that led to it, and a
    // pre-release never supersedes the finished version.
    match (&c.pre, &l.pre) {
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (Some(a), Some(b)) => b > a,
        (None, None) => false,
    }
}

/// How this build was installed.
///
/// `APPIMAGE` is set by the AppImage runtime itself, which is the only
/// trustworthy signal available from inside the process.
pub fn install_kind() -> InstallKind {
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallKind::AppImage;
    }
    if cfg!(debug_assertions) {
        return InstallKind::Source;
    }
    InstallKind::Package
}

/// The tag name from a GitHub "latest release" response.
fn tag_from(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    serde_json::from_str::<Release>(body)
        .ok()
        .map(|r| r.tag_name)
}

/// Ask GitHub what the newest published release is.
///
/// Failure is not an error the user should see. No network, a rate-limited
/// API, or GitHub being down all mean "we do not know", and an app that
/// complains about its own update check every time someone opens it offline is
/// worse than one that quietly says nothing.
pub fn check(current: &str) -> AppUpdateView {
    let kind = install_kind();
    let mut view = AppUpdateView {
        current: current.to_string(),
        latest: None,
        available: false,
        url: RELEASES_PAGE.to_string(),
        install_kind: kind,
    };

    // GitHub refuses requests without a User-Agent, so it is set explicitly
    // rather than left to the HTTP library's default.
    let res = ureq::get(RELEASES_API)
        .set("User-Agent", &format!("Apocrypha/{current}"))
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call();

    let Ok(res) = res else { return view };
    let Ok(body) = res.into_string() else {
        return view;
    };
    let Some(tag) = tag_from(&body) else {
        return view;
    };

    view.available = is_newer(current, &tag);
    view.latest = Some(tag);
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_version_is_newer() {
        assert!(is_newer("0.3.0", "0.4.0"));
        assert!(is_newer("0.4.0", "0.4.1"));
        assert!(is_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        assert!(!is_newer("0.4.0", "0.4.0"));
    }

    #[test]
    fn an_older_release_is_not_an_update() {
        // A tag can move or be republished; going backwards must never be
        // offered as an upgrade.
        assert!(!is_newer("0.4.0", "0.3.0"));
        assert!(!is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn the_v_prefix_on_tags_is_ignored() {
        // Tags are `v0.4.0`, Cargo reports `0.4.0`. Comparing them literally
        // would make every check claim an update.
        assert!(is_newer("0.3.0", "v0.4.0"));
        assert!(!is_newer("0.4.0", "v0.4.0"));
        assert!(!is_newer("v0.4.0", "0.4.0"));
    }

    #[test]
    fn numbers_are_compared_as_numbers_not_text() {
        // The classic bug: "0.10.0" sorts before "0.9.0" as a string.
        assert!(is_newer("0.9.0", "0.10.0"));
        assert!(!is_newer("0.10.0", "0.9.0"));
        assert!(is_newer("1.2.9", "1.2.10"));
    }

    #[test]
    fn a_release_supersedes_its_own_pre_release() {
        assert!(is_newer("0.5.0-beta.1", "0.5.0"));
        assert!(!is_newer("0.5.0", "0.5.0-beta.1"));
    }

    #[test]
    fn a_later_pre_release_supersedes_an_earlier_one() {
        assert!(is_newer("0.5.0-beta.1", "0.5.0-beta.2"));
        assert!(!is_newer("0.5.0-beta.2", "0.5.0-beta.1"));
    }

    #[test]
    fn a_short_tag_is_understood() {
        assert!(is_newer("0.4.0", "0.5"));
        assert!(!is_newer("0.5.0", "0.5"));
    }

    #[test]
    fn build_metadata_does_not_affect_precedence() {
        assert!(!is_newer("0.4.0", "0.4.0+20260810"));
    }

    #[test]
    fn nonsense_never_claims_an_update() {
        // A malformed tag must not be read as "something newer exists".
        assert!(!is_newer("0.4.0", "latest"));
        assert!(!is_newer("0.4.0", ""));
        assert!(!is_newer("", "0.4.0"));
        assert!(!is_newer("nightly", "also-nightly"));
    }

    #[test]
    fn a_tag_is_read_out_of_a_real_response_shape() {
        let body = r#"{"tag_name":"v0.4.0","name":"Apocrypha 0.4.0","draft":false}"#;
        assert_eq!(tag_from(body).as_deref(), Some("v0.4.0"));
    }

    #[test]
    fn a_response_without_a_tag_yields_nothing() {
        assert_eq!(tag_from(r#"{"message":"Not Found"}"#), None);
        assert_eq!(tag_from("not json at all"), None);
    }
}
