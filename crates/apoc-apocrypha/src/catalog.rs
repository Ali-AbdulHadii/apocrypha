//! Reading the Apocrypha catalogue.
//!
//! Browsing is a read the service performs on the account's behalf, so the
//! device token goes on every request and the service decides what comes back.
//! Nothing here filters: a client that hides a mod the server sent is a client
//! whose filter can be turned off, and visibility — scan state, adult content,
//! region — is the server's to enforce. The app displays what it is given.

use serde::{Deserialize, Serialize};

use crate::{agent, pairing::PairingError, ServiceOrigin};

/// One mod in a listing.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMod {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub author_name: String,
    pub game_slug: String,
    pub game_name: String,
    pub download_count: i64,
    pub favor_count: i64,
    #[serde(default)]
    pub latest_version: Option<String>,
}

/// A page of results, in the service's envelope.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub items: Vec<CatalogMod>,
    pub page: i32,
    pub page_size: i32,
    pub total: i64,
}

impl CatalogPage {
    /// Whether asking for the next page is worth a request.
    pub fn has_more(&self) -> bool {
        (self.page as i64) * (self.page_size as i64) < self.total
    }
}

/// A game as the *service* lists it.
///
/// Its slug is the service's, which is not this app's game id. The two
/// catalogues are maintained separately, and a game this app can manage may not
/// be listed there at all — so anything filtering by game matches against this
/// rather than assuming the two identifier spaces agree. Today they happen to,
/// which is exactly the kind of coincidence that stops being true without
/// anyone noticing.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGame {
    pub id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub mod_count: i64,
}

/// One downloadable file on a release.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFile {
    pub id: String,
    pub file_name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub size_bytes: i64,
    /// Hex SHA-256 of the bytes, as the service recorded them at upload.
    pub sha256: String,
    pub scan_state: String,
    pub upload_state: String,
}

impl CatalogFile {
    /// What to show a person: the creator's own label if they set one.
    pub fn label(&self) -> &str {
        self.display_name
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(&self.file_name)
    }

    /// Whether the service will actually hand these bytes over.
    ///
    /// The server decides this too and refuses regardless, so this is only
    /// about not offering a button that is going to fail. Both conditions are
    /// required: a file is served only once scanning reports clean *and* its
    /// bytes were verified against the hash recorded at upload.
    pub fn is_downloadable(&self) -> bool {
        self.scan_state.eq_ignore_ascii_case("Clean")
            && self.upload_state.eq_ignore_ascii_case("Verified")
    }
}

/// One release of a mod.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersion {
    pub id: String,
    pub version_number: String,
    #[serde(default)]
    pub changelog: String,
    pub release_channel: String,
    pub is_latest: bool,
    #[serde(default)]
    pub files: Vec<CatalogFile>,
}

/// A mod with its releases, as the detail endpoint returns it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModDetail {
    pub id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    pub author_name: String,
    pub game_slug: String,
    pub game_name: String,
    #[serde(default)]
    pub download_count: i64,
    #[serde(default)]
    pub favor_count: i64,
    /// What this mod needs, and what it cannot sit beside.
    ///
    /// Carried because installing a mod that requires another, or that is known
    /// to break one already deployed, is the failure a person most wants
    /// warning about *before* the download rather than after the game refuses
    /// to start.
    #[serde(default)]
    pub relationships: Vec<CatalogRelationship>,
    #[serde(default)]
    pub versions: Vec<CatalogVersion>,
}

/// A stated link between two mods, as the service records it.
///
/// `kind` is the service's own vocabulary and is deliberately kept as text: the
/// catalogue can add a relationship type without this client refusing to parse a
/// mod page, and an unknown kind is shown as itself rather than dropped.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRelationship {
    pub target_mod_id: String,
    pub target_mod_slug: String,
    pub target_mod_name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

impl CatalogRelationship {
    /// Whether this is something the mod needs in order to work.
    ///
    /// Matched case-insensitively against the service's own spelling
    /// (`Required`, `Optional`, `Incompatible`) rather than a guess at it: a
    /// predicate that quietly matches nothing is worse than none, because the
    /// warning it feeds simply never appears.
    pub fn is_required(&self) -> bool {
        self.kind.eq_ignore_ascii_case("required")
    }

    /// Something the mod can use but does not need.
    pub fn is_optional(&self) -> bool {
        self.kind.eq_ignore_ascii_case("optional")
    }

    /// Something the mod is known not to work beside.
    pub fn is_incompatible(&self) -> bool {
        self.kind.eq_ignore_ascii_case("incompatible")
    }
}

impl CatalogModDetail {
    /// The release to offer first.
    ///
    /// `isLatest` is the service's own answer and is preferred over guessing
    /// from version strings, which are creator-authored text and do not sort.
    pub fn latest(&self) -> Option<&CatalogVersion> {
        self.versions
            .iter()
            .find(|v| v.is_latest)
            .or_else(|| self.versions.first())
    }
}

/// Permission to fetch one file, granted for a short window.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTicket {
    /// Where to GET the bytes. A bearer capability, not a stable address:
    /// it must not be logged, stored or shown.
    pub url: String,
    pub expires_at: String,
    pub file_name: String,
    pub size_bytes: i64,
}

/// The account's standing with the daily cap.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadQuota {
    pub verified: bool,
    pub distinct_mods_today: i32,
    #[serde(default)]
    pub daily_limit: Option<i32>,
    #[serde(default)]
    pub remaining: Option<i32>,
}

pub struct Catalog {
    origin: ServiceOrigin,
    app_version: String,
    token: String,
}

impl Catalog {
    pub fn new(
        origin: ServiceOrigin,
        app_version: impl Into<String>,
        token: impl Into<String>,
    ) -> Self {
        Self {
            origin,
            app_version: app_version.into(),
            token: token.into(),
        }
    }

    /// A page of published mods, optionally narrowed to one game or a search.
    ///
    /// `game` is the service's own slug, which is not this app's game id: the
    /// two catalogues are independent and a profile here may have no listing
    /// there. The caller passes whatever the service returned, never a local id.
    pub fn mods(
        &self,
        game: Option<&str>,
        search: Option<&str>,
        page: u32,
        page_size: u32,
    ) -> Result<CatalogPage, PairingError> {
        let mut url = format!(
            "{}/api/v1/mods?page={}&pageSize={}",
            self.origin.as_str(),
            page.max(1),
            page_size.clamp(1, 100),
        );
        if let Some(g) = game.filter(|g| !g.is_empty()) {
            url.push_str(&format!("&game={}", encode(g)));
        }
        if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
            url.push_str(&format!("&search={}", encode(s.trim())));
        }

        let response = agent()
            .get(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .call();

        match response {
            Ok(res) => res
                .into_json::<CatalogPage>()
                .map_err(|e| PairingError::Decode(e.to_string())),
            Err(e) => Err(map_error(e, "browse the catalogue")),
        }
    }

    /// Every game the service lists.
    ///
    /// Small, stable, and worth asking for before filtering by game: it is the
    /// only way to tell "this game has no mods yet" from "the service has never
    /// heard of this game", and those need different words on screen.
    pub fn games(&self) -> Result<Vec<CatalogGame>, PairingError> {
        let url = format!("{}/api/v1/games", self.origin.as_str());
        self.get(&url, "read the game list")?
            .into_json::<Vec<CatalogGame>>()
            .map_err(|e| PairingError::Decode(e.to_string()))
    }

    /// One mod with its releases and their files.
    ///
    /// Addressed by the pair of slugs the service itself returned, never by
    /// anything assembled here — the listing gives both, and the detail route
    /// is the only place file ids come from.
    pub fn mod_detail(
        &self,
        game_slug: &str,
        mod_slug: &str,
    ) -> Result<CatalogModDetail, PairingError> {
        let url = format!(
            "{}/api/v1/games/{}/mods/{}",
            self.origin.as_str(),
            encode(game_slug),
            encode(mod_slug),
        );

        self.get(&url, "read this mod")?
            .into_json::<CatalogModDetail>()
            .map_err(|e| PairingError::Decode(e.to_string()))
    }

    /// The account's standing with the daily cap.
    pub fn download_quota(&self) -> Result<DownloadQuota, PairingError> {
        let url = format!("{}/api/v1/downloads/quota", self.origin.as_str());
        self.get(&url, "check the download allowance")?
            .into_json::<DownloadQuota>()
            .map_err(|e| PairingError::Decode(e.to_string()))
    }

    /// Claim a download of one file.
    ///
    /// A POST because it is a write on the service's side: it spends a slot
    /// against the daily cap and moves the mod's counter. Calling it twice for
    /// one mod in a day costs one slot, not two, but it is still not something
    /// to call speculatively — the button press is the intent.
    pub fn claim_download(&self, file_id: &str) -> Result<DownloadTicket, PairingError> {
        let url = format!(
            "{}/api/v1/downloads/files/{}",
            self.origin.as_str(),
            encode(file_id),
        );

        let response = agent()
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .set("Content-Length", "0")
            .call();

        match response {
            Ok(res) => res
                .into_json::<DownloadTicket>()
                .map_err(|e| PairingError::Decode(e.to_string())),
            Err(ureq::Error::Status(409, res)) => {
                // The daily cap, or a file the service will not serve. Its own
                // words are better than anything guessed from a status code —
                // "you have reached five mods today" is actionable and
                // "conflict" is not.
                Err(PairingError::Refused(service_message(res).unwrap_or_else(
                    || "That file cannot be downloaded right now.".into(),
                )))
            }
            Err(e) => Err(map_error(e, "download this file")),
        }
    }

    /// A signed GET, with failures already turned into something showable.
    ///
    /// The mapping happens here rather than at each call site because
    /// `ureq::Error` is 272 bytes and returning it from a helper makes every
    /// `Result` in the chain that size — which clippy refuses, correctly.
    fn get(&self, url: &str, what: &str) -> Result<ureq::Response, PairingError> {
        agent()
            .get(url)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
            .call()
            .map_err(|e| map_error(e, what))
    }
}

/// Turns a transport or status failure into something worth showing.
///
/// `what` completes the sentence "This computer is not allowed to ...", so the
/// refusal names the thing that was refused rather than being one message for
/// every endpoint. 403 in particular is the one a device hits when it was
/// paired before a scope existed, and telling someone their computer cannot
/// browse when it actually cannot download would send them to the wrong fix.
fn map_error(e: ureq::Error, what: &str) -> PairingError {
    match e {
        ureq::Error::Status(401, _) => {
            PairingError::Refused("This computer is not signed in any more. Sign in again.".into())
        }
        ureq::Error::Status(403, _) => PairingError::Refused(format!(
            "This computer is not allowed to {what}. Signing in again may fix it."
        )),
        ureq::Error::Status(404, _) => {
            PairingError::Refused("That is not available on the service.".into())
        }
        ureq::Error::Status(code, _) => PairingError::Unexpected(code),
        e => PairingError::Http(e.to_string()),
    }
}

/// The service's own `{ "error": "..." }` message, when it sent one.
fn service_message(res: ureq::Response) -> Option<String> {
    #[derive(Deserialize)]
    struct Body {
        error: Option<String>,
    }
    res.into_json::<Body>()
        .ok()
        .and_then(|b| b.error)
        .filter(|m| !m.trim().is_empty())
}

/// Percent-encodes a query value.
///
/// Search text is whatever someone typed, so it can contain `&`, `=`, `#` and
/// spaces — all of which would otherwise change which parameters the service
/// sees rather than what it searches for.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_cannot_smuggle_extra_parameters() {
        // The obvious failure: someone types an ampersand and the service reads
        // the rest of it as a different filter.
        assert_eq!(encode("body&game=cyberpunk"), "body%26game%3Dcyberpunk");
        assert_eq!(encode("hello world"), "hello%20world");
        assert_eq!(encode("re:framework#1"), "re%3Aframework%231");
        assert_eq!(encode("plain-text_1.0~x"), "plain-text_1.0~x");
    }

    #[test]
    fn paging_knows_when_to_stop() {
        let page = |page, page_size, total| CatalogPage {
            items: vec![],
            page,
            page_size,
            total,
        };
        assert!(page(1, 20, 100).has_more());
        assert!(
            !page(5, 20, 100).has_more(),
            "the last full page is the end"
        );
        assert!(
            !page(1, 20, 0).has_more(),
            "an empty catalogue has no next page"
        );
        assert!(page(1, 20, 21).has_more());
    }

    #[test]
    fn a_listing_parses_the_services_envelope() {
        let body = r#"{
          "items": [{
            "id": "3f", "slug": "reframework", "name": "REFramework",
            "summary": "Scripting platform", "authorName": "praydog",
            "state": "Published", "gameSlug": "monster-hunter-wilds",
            "gameName": "Monster Hunter Wilds", "downloadCount": 2645330,
            "favorCount": 27828, "latestVersion": "Nightly01240",
            "updatedAt": "2026-08-09T00:00:00+00:00"
          }],
          "page": 1, "pageSize": 20, "total": 2
        }"#;
        let parsed: CatalogPage = serde_json::from_str(body).expect("parses");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].name, "REFramework");
        assert_eq!(parsed.items[0].author_name, "praydog");
        assert_eq!(
            parsed.items[0].latest_version.as_deref(),
            Some("Nightly01240")
        );
    }

    #[test]
    fn relationships_are_classified_by_the_services_own_spelling() {
        let rel = |kind: &str| CatalogRelationship {
            target_mod_id: "1".into(),
            target_mod_slug: "s".into(),
            target_mod_name: "N".into(),
            kind: kind.into(),
        };

        // These are the exact three the service emits. Matching invented
        // synonyms instead would leave every predicate false, and a warning
        // that never fires is worse than no warning: the screen looks like it
        // checked.
        assert!(rel("Required").is_required());
        assert!(rel("Optional").is_optional());
        assert!(rel("Incompatible").is_incompatible());

        assert!(rel("required").is_required(), "casing is not a contract");

        // Each is only itself.
        assert!(!rel("Optional").is_required());
        assert!(!rel("Required").is_incompatible());

        // A kind added to the catalogue later must not be silently read as one
        // of these; it falls through and is shown as itself.
        let unknown = rel("Supersedes");
        assert!(!unknown.is_required() && !unknown.is_optional() && !unknown.is_incompatible());
    }

    #[test]
    fn a_files_readiness_needs_both_a_clean_scan_and_verified_bytes() {
        let file = |scan: &str, upload: &str| CatalogFile {
            id: "1".into(),
            file_name: "mod.zip".into(),
            display_name: None,
            size_bytes: 10,
            sha256: "ab".into(),
            scan_state: scan.into(),
            upload_state: upload.into(),
        };
        assert!(file("Clean", "Verified").is_downloadable());
        assert!(
            !file("Pending", "Verified").is_downloadable(),
            "not scanned yet"
        );
        assert!(
            !file("Clean", "Uploaded").is_downloadable(),
            "bytes unverified"
        );
        assert!(!file("Infected", "Verified").is_downloadable());
        // The service spells these in PascalCase; a client that only matched
        // one casing would silently offer nothing.
        assert!(file("clean", "verified").is_downloadable());
    }

    #[test]
    fn a_file_shows_its_creators_label_when_there_is_one() {
        let mut f = CatalogFile {
            id: "1".into(),
            file_name: "ver-r-body-1.03.zip".into(),
            display_name: Some("Main file".into()),
            size_bytes: 1,
            sha256: String::new(),
            scan_state: "Clean".into(),
            upload_state: "Verified".into(),
        };
        assert_eq!(f.label(), "Main file");
        f.display_name = Some("   ".into());
        assert_eq!(f.label(), "ver-r-body-1.03.zip", "blank is not a label");
        f.display_name = None;
        assert_eq!(f.label(), "ver-r-body-1.03.zip");
    }

    #[test]
    fn the_service_decides_which_release_is_latest() {
        let version = |num: &str, latest: bool| CatalogVersion {
            id: num.into(),
            version_number: num.into(),
            changelog: String::new(),
            release_channel: "Stable".into(),
            is_latest: latest,
            files: vec![],
        };
        let detail = |versions| CatalogModDetail {
            id: "1".into(),
            slug: "s".into(),
            name: "n".into(),
            summary: String::new(),
            description: String::new(),
            author_name: "a".into(),
            game_slug: "g".into(),
            game_name: "G".into(),
            download_count: 0,
            favor_count: 0,
            relationships: vec![],
            versions,
        };

        // Deliberately not the highest-looking string: version numbers are
        // creator-authored text and do not sort, so the flag is the answer.
        let d = detail(vec![version("1.10", false), version("1.9", true)]);
        assert_eq!(d.latest().unwrap().version_number, "1.9");

        // Nothing flagged: fall back to the first rather than to nothing.
        let d = detail(vec![version("2.0", false)]);
        assert_eq!(d.latest().unwrap().version_number, "2.0");

        assert!(detail(vec![]).latest().is_none());
    }

    #[test]
    fn a_detail_response_parses_with_its_files() {
        let body = r#"{
          "id":"3f","slug":"reframework","name":"REFramework","summary":"",
          "description":"","authorName":"praydog","state":"Published",
          "gameSlug":"monster-hunter-wilds","gameName":"Monster Hunter Wilds",
          "downloadCount":1,"favorCount":1,
          "createdAt":"2026-08-09T00:00:00+00:00","updatedAt":"2026-08-09T00:00:00+00:00",
          "firstPublishedAt":null,"categories":[],"tags":[],"relationships":[],
          "versions":[{
            "id":"v1","versionNumber":"Nightly01240","changelog":"","releaseChannel":"Stable",
            "compatibilityNotes":"","installationNotes":"","knownIssues":"","license":"",
            "gameVersions":[],"storefronts":[],"loaders":[],"isLatest":true,
            "releasedAt":"2026-08-09T00:00:00+00:00",
            "files":[{
              "id":"f1","fileName":"REFramework.zip","displayName":null,
              "sizeBytes":4194304,"sha256":"deadbeef","uploadState":"Verified",
              "scanState":"Clean","integrityVerifiedAt":"2026-08-09T00:00:00+00:00"
            }]
          }]
        }"#;
        let parsed: CatalogModDetail = serde_json::from_str(body).expect("parses");
        let latest = parsed.latest().expect("has a release");
        assert_eq!(latest.version_number, "Nightly01240");
        assert_eq!(latest.files.len(), 1);
        assert!(latest.files[0].is_downloadable());
        assert_eq!(latest.files[0].sha256, "deadbeef");
    }

    #[test]
    fn a_ticket_parses() {
        let body = r#"{"url":"https://example.invalid/x?sig=1",
          "expiresAt":"2026-08-10T00:00:00+00:00","fileName":"m.zip","sizeBytes":42}"#;
        let t: DownloadTicket = serde_json::from_str(body).expect("parses");
        assert_eq!(t.size_bytes, 42);
        assert_eq!(t.file_name, "m.zip");
    }

    #[test]
    fn an_uncapped_quota_parses_as_no_limit() {
        // A verified account has no cap, and the service says so with nulls
        // rather than with a large number.
        let body = r#"{"verified":true,"distinctModsToday":3,"dailyLimit":null,"remaining":null}"#;
        let q: DownloadQuota = serde_json::from_str(body).expect("parses");
        assert!(q.verified);
        assert!(q.daily_limit.is_none());
        assert_eq!(q.distinct_mods_today, 3);
    }

    #[test]
    fn a_mod_without_a_release_still_parses() {
        // latestVersion is null for a mod with no published version, and a
        // listing that refuses to parse would take the whole page down.
        let body = r#"{"items":[{"id":"1","slug":"s","name":"n","summary":"",
          "authorName":"a","state":"Published","gameSlug":"g","gameName":"G",
          "downloadCount":0,"favorCount":0,"latestVersion":null,
          "updatedAt":"2026-08-09T00:00:00+00:00"}],"page":1,"pageSize":20,"total":1}"#;
        let parsed: CatalogPage = serde_json::from_str(body).expect("parses");
        assert!(parsed.items[0].latest_version.is_none());
    }
}
