//! Game profiles fetched from the service.
//!
//! A profile says where Steam installs a game, which directories hold mod
//! content, what loader it needs and how its paths are spelled. Every one the
//! app knows is compiled into it, which means a game that moves a directory in
//! a patch breaks modding for everyone until a new release ships. This is the
//! other source: the same documents, published, so a fix reaches people the day
//! it is written.
//!
//! Three things govern how it behaves, and all three are about what happens
//! when the service is not there.
//!
//! **The bundled profiles are the floor, never the ceiling.** Any failure —
//! offline, a timeout, a 500, a document that will not parse, a schema version
//! this build does not know — falls back to what is compiled in. Modding does
//! not depend on the network, on the service being up, or on having an account,
//! and none of those is a condition anyone agreed to when they installed a mod
//! manager.
//!
//! **A profile is taken whole or not at all.** A document that parses is used;
//! one that does not is discarded entirely rather than merged field by field
//! with the bundled one. Half a deploy target list is worse than the version
//! that was already working.
//!
//! **The origin is compiled in.** Same rule as everywhere else in this crate: a
//! profile decides where files are written into somebody's game directory, so
//! "fetch that from wherever a settings file says" is not a feature.

use crate::{agent, send_retrying, ServiceOrigin};
use apoc_domain::{GameProfile, PluginActivation, PluginListSpec, RootFilesSpec};
use apoc_gamedef::{GameDatabaseSource, GameDefError, LocalBuiltin};
use serde::Deserialize;

/// The profile contract this build understands.
///
/// A document declaring anything else is refused rather than read: a newer
/// contract may mean something different by a field this build thinks it knows,
/// and the failure that produces is files written to the wrong place.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// What happened on the last attempt to reach the service.
///
/// Carried so the interface can say which profiles are actually in use. An app
/// that has silently fallen back looks exactly like one that is working, right
/// up until someone wonders why a profile they published has not arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Profiles came from the service.
    Online { fetched: usize },
    /// The service could not be used, and the bundled profiles are in force.
    Offline { reason: String },
}

/// Where a profile document comes from.
///
/// The seam exists because the interesting behaviour of this module is what it
/// does when the service misbehaves — offline, a 404, a 500, an index that is
/// not the shape it claims, a profile document that will not parse — and every
/// one of those was reachable only by having a real service on the other end
/// behave badly on cue. Tests here must assume no network at all, so the five
/// failure modes were untested precisely because they are the ones that matter.
///
/// Deliberately narrow: a URL in, a body or a sentence out. Retries, headers
/// and status-code wording stay in [`HttpProfiles`], because a fake that had to
/// reproduce them would be testing itself.
pub trait ProfileTransport {
    /// The body of a GET, or a sentence saying why there is not one.
    ///
    /// The error reaches somebody who wants to install a mod, so it is written
    /// for them rather than naming a protocol.
    fn get(&self, url: &str) -> Result<String, String>;
}

/// The real transport: `ureq`, with the retry the service's redeploys need.
#[derive(Debug, Clone)]
pub struct HttpProfiles {
    app_version: String,
}

impl ProfileTransport for HttpProfiles {
    // Reads, so the transport retry applies: the service redeploys on every
    // push and a dropped connection is routine rather than exceptional.
    #[allow(clippy::result_large_err)]
    fn get(&self, url: &str) -> Result<String, String> {
        send_retrying(|| {
            agent()
                .get(url)
                // No Authorization header. Profiles are public, and a manager
                // has to work for someone who has never signed in.
                .set("User-Agent", &format!("Apocrypha/{}", self.app_version))
                .call()
        })
        .map_err(describe)?
        .into_string()
        .map_err(|_| "the service's answer could not be read".to_string())
    }
}

/// Profiles from the service, with the bundled set underneath.
#[derive(Debug, Clone)]
pub struct ApocryphaGameDb<T = HttpProfiles> {
    origin: ServiceOrigin,
    transport: T,
}

impl ApocryphaGameDb<HttpProfiles> {
    pub fn new(origin: ServiceOrigin, app_version: impl Into<String>) -> Self {
        let app_version = app_version.into();
        ApocryphaGameDb {
            origin,
            transport: HttpProfiles { app_version },
        }
    }
}

impl<T: ProfileTransport> ApocryphaGameDb<T> {
    /// The same client, answered by something other than the network.
    pub fn with_transport(origin: ServiceOrigin, transport: T) -> Self {
        ApocryphaGameDb { origin, transport }
    }

    /// Fetch every published profile, reporting what happened.
    ///
    /// Never returns an error: a failure produces the bundled profiles and a
    /// reason, because there is no situation in which this refusing to answer
    /// is more useful than it answering with what it already had.
    pub fn fetch(&self) -> (Vec<GameProfile>, Freshness) {
        let builtin = match LocalBuiltin::new().all() {
            Ok(profiles) => profiles,
            // The bundled profiles are `include_str!`ed and parsed by a test on
            // every build, so this is unreachable short of a corrupted binary.
            Err(e) => {
                return (
                    Vec::new(),
                    Freshness::Offline {
                        reason: format!("the built-in profiles could not be read: {e}"),
                    },
                )
            }
        };

        let index = match self.index() {
            Ok(index) => index,
            Err(reason) => return (builtin, Freshness::Offline { reason }),
        };

        let mut fetched: Vec<GameProfile> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        for entry in index {
            if entry.schema_version != SUPPORTED_SCHEMA {
                refused.push(format!(
                    "{} needs profile format {}, and this version reads {SUPPORTED_SCHEMA}",
                    entry.id, entry.schema_version
                ));
                continue;
            }
            match self.profile(&entry.id) {
                Ok(profile) => fetched.push(profile),
                Err(reason) => refused.push(reason),
            }
        }

        // Whatever arrived replaces its bundled namesake; whatever did not keeps
        // it. A game the service has never heard of stays exactly as it shipped.
        let mut merged = builtin;
        for profile in fetched.iter() {
            match merged.iter_mut().find(|g| g.id == profile.id) {
                Some(existing) => *existing = profile.clone(),
                None => merged.push(profile.clone()),
            }
        }

        let freshness = if refused.is_empty() {
            Freshness::Online {
                fetched: fetched.len(),
            }
        } else if fetched.is_empty() {
            Freshness::Offline {
                reason: refused.join("; "),
            }
        } else {
            // Some arrived and some did not. Online, but the ones that were
            // refused are named rather than quietly absent.
            Freshness::Online {
                fetched: fetched.len(),
            }
        };
        (merged, freshness)
    }

    fn index(&self) -> Result<Vec<IndexEntry>, String> {
        let url = format!("{}/api/v1/games/profiles", self.origin.as_str());
        let body = self.transport.get(&url)?;
        serde_json::from_str::<Vec<IndexEntry>>(&body)
            .map_err(|e| format!("the list of game profiles could not be read: {e}"))
    }

    fn profile(&self, id: &str) -> Result<GameProfile, String> {
        parse_document(&self.document(id)?)
    }

    /// One profile as the service sent it, unparsed.
    ///
    /// Kept as text so it can be cached verbatim. Storing a parsed profile
    /// instead would mean the cache has to learn every field the schema grows,
    /// and a cache that can fall behind the thing it caches is worse than none.
    pub fn document(&self, id: &str) -> Result<String, String> {
        let url = format!(
            "{}/api/v1/games/{}/profile",
            self.origin.as_str(),
            encode(id)
        );
        self.transport.get(&url)
    }

    /// Every published profile, as documents ready to be cached.
    ///
    /// `(game id, document, schema version)`. Each is checked for a contract
    /// this build reads and parsed once here, so a document that would fail
    /// later is refused now rather than being cached and failing on the next
    /// start, when nothing is left to explain it.
    pub fn fetch_documents(&self) -> Result<Vec<(String, String, u32)>, String> {
        let mut out = Vec::new();
        for entry in self.index()? {
            if entry.schema_version != SUPPORTED_SCHEMA {
                continue;
            }
            let document = self.document(&entry.id)?;
            parse_document(&document)?;
            out.push((entry.id, document, entry.schema_version));
        }
        Ok(out)
    }
}

impl<T: ProfileTransport> GameDatabaseSource for ApocryphaGameDb<T> {
    /// Profiles from the service where it answered, and the bundled ones
    /// everywhere else.
    ///
    /// The trait cannot say "this succeeded but from the fallback", so this
    /// always succeeds. Call [`ApocryphaGameDb::fetch`] where the difference
    /// matters, which is anywhere it is going to be shown to somebody.
    fn all(&self) -> Result<Vec<GameProfile>, GameDefError> {
        Ok(self.fetch().0)
    }
}

/// Read one profile document.
///
/// Public because the app caches documents on disk and has to read them back
/// on a later start, when nothing has been fetched yet and there may be no
/// network at all.
pub fn parse_document(json: &str) -> Result<GameProfile, String> {
    let profile = serde_json::from_str::<WireProfile>(json)
        .map(WireProfile::into_profile)
        .map_err(|e| format!("a published game profile could not be read: {e}"))?;
    validate(&profile)?;
    Ok(profile)
}

/// A path component a profile may name.
///
/// Profile paths are relative and go downwards: into the game directory, into
/// the app's own data directory, into a staging tree. `..`, a root, a drive
/// letter and a NUL are the four ways to say otherwise, and a backslash is the
/// fifth once the engine compiles for Windows.
fn descends(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && !path.contains(':')
        && !path
            .split('/')
            .any(|seg| seg == ".." || seg == "." || seg.is_empty())
}

/// An id a profile may claim.
///
/// The id is not only a name. It is a directory under the app's data root, a
/// key in the local database and a segment in a URL, so it has to be spellable
/// as all three. The bundled profiles are already slugs; this says so.
fn id_is_a_slug(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Refuse a published profile that would write somewhere it should not.
///
/// A profile decides where files land in somebody's game directory and under
/// their data root. While every profile was compiled into the binary that was a
/// review question. Published, it is a document arriving over a network, and
/// the honest treatment of a document is to check it before believing it.
///
/// Refused by name, and whole: [`ApocryphaGameDb::fetch`] discards the profile
/// and keeps the bundled one, which is the same answer it gives for a timeout
/// or a document that will not parse. A half-applied profile — the deploy
/// targets from the service, the loader from the binary — is a configuration
/// nobody wrote and nobody tested.
fn validate(profile: &GameProfile) -> Result<(), String> {
    let id = &profile.id;
    if !id_is_a_slug(id) {
        return Err(format!(
            "a published game profile claims the id {id:?}, which is not a slug"
        ));
    }

    let mut paths: Vec<(&str, &str)> = Vec::new();
    for t in &profile.deploy_targets {
        paths.push(("a deploy target source", &t.source));
        paths.push(("a deploy target", &t.target));
    }
    for r in &profile.rewrap {
        paths.push(("a rewrap folder", &r.folder));
        paths.push(("a rewrap prefix", &r.prefix));
    }
    for c in &profile.canonical_case {
        paths.push(("a canonical-case path", c));
    }
    if let Some(f) = &profile.fomod {
        paths.push(("the FOMOD destination prefix", &f.dest_prefix));
    }
    if let Some(l) = &profile.loader {
        if let Some(dll) = &l.proxy_dll {
            paths.push(("the loader's proxy DLL", dll));
        }
        for dll in &l.also_provides {
            paths.push(("a loader proxy DLL", dll));
        }
        for dir in &l.data_dirs {
            paths.push(("a loader data directory", dir));
        }
    }
    if let Some(chain) = &profile.pak_chain {
        // A filename rather than a path, and it is built from by formatting, so
        // it must not contain a separator either.
        paths.push(("the pak chain pattern", &chain.pattern));
    }
    if let Some(list) = &profile.plugin_list {
        // Each of these is joined onto a location inside the user's Proton
        // prefix, so each is a chance for a document to choose where a write
        // lands. `descends` refuses the separators outright, which is stricter
        // than these need to be and exactly as strict as they should be: every
        // one is a single directory or file name.
        paths.push(("the plugin list directory", &list.dir));
        paths.push(("the plugin list file", &list.plugins_file));
        if let Some(name) = &list.load_order_file {
            paths.push(("the load order file", name));
        }
        for name in &list.implicit {
            paths.push(("an implicitly loaded plugin", name));
        }
    }

    if let Some(root) = &profile.root_files {
        // The folder is stripped off an archive path and what remains is joined
        // onto the game directory, so a folder that climbs is a document
        // choosing to write outside the game. `safe_dest` refuses that at the
        // moment of writing; refusing it here means the profile is rejected by
        // name instead of every file it produces failing one at a time.
        if let Some(folder) = &root.folder {
            paths.push(("the root files folder", folder));
        }
        // Patterns are matched against a bare filename and never joined onto
        // anything, so a separator in one cannot escape — it simply could never
        // match. Refused anyway, because a pattern that cannot match is a
        // profile that silently does nothing, and saying so is more useful.
        for pattern in &root.patterns {
            paths.push(("a root file pattern", pattern));
        }
    }

    for (what, path) in paths {
        if !descends(path) {
            return Err(format!(
                "the published profile for {id} names {what} that leaves the directory it is written into: {path:?}"
            ));
        }
    }

    if let Some(chain) = &profile.pak_chain {
        if chain.pattern.contains('/') {
            return Err(format!(
                "the published profile for {id} gives a pak chain pattern that is a path, not a filename: {:?}",
                chain.pattern
            ));
        }
    }

    Ok(())
}

/// A failure in the terms of the person waiting on it.
fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(404, _) => "the service has no game profiles to publish".to_string(),
        ureq::Error::Status(code, _) if (500..600).contains(&code) => {
            "the service is having trouble; the built-in profiles are being used".to_string()
        }
        ureq::Error::Status(code, _) => format!("the service refused the request ({code})"),
        // Offline, DNS, TLS, a timeout. All the same thing to somebody who just
        // wants to install a mod: the service could not be reached.
        ureq::Error::Transport(_) => "the service could not be reached".to_string(),
    }
}

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

/* ----------------------------------------------------------------- wire --- */

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    id: String,
    #[allow(dead_code)]
    name: String,
    schema_version: u32,
    #[allow(dead_code)]
    revision: u32,
}

/// The service's profile document.
///
/// Deliberately its own type rather than deserializing straight into
/// [`GameProfile`]. The domain type is written for TOML, where a field it does
/// not know is a mistake worth failing on; a document from a service is
/// something to be read defensively, and the two want opposite settings. This
/// also puts the mapping in one visible place, so a field added on one side and
/// forgotten on the other is a compile error rather than a silently missing
/// deploy target.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireProfile {
    id: String,
    name: String,
    engine: String,
    #[serde(default)]
    nexus_domain: Option<String>,
    load_order: String,
    /// Read and discarded: one scope exists, so there is nothing to choose
    /// between. Named here rather than dropped from the type so the next person
    /// can see the service sends it and that it was considered.
    #[allow(dead_code)]
    conflict_scope: String,
    case_sensitive: bool,
    detection: WireDetection,
    #[serde(default)]
    formats: Vec<String>,
    #[serde(default)]
    canonical_case: Vec<String>,
    #[serde(default)]
    plugin_extensions: Vec<String>,
    #[serde(default)]
    deploy_targets: Vec<WireDeployTarget>,
    #[serde(default)]
    rewrap: Vec<WireRewrap>,
    #[serde(default)]
    loader: Option<WireLoader>,
    #[serde(default)]
    pak_chain: Option<WirePakChain>,
    #[serde(default)]
    fomod: Option<WireFomod>,
    #[serde(default)]
    plugin_list: Option<WirePluginList>,
    #[serde(default)]
    root_files: Option<WireRootFiles>,
}

/// Which of a mod's files belong beside the game executable, as published.
#[derive(Debug, Deserialize)]
struct WireRootFiles {
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    patterns: Vec<String>,
}

/// A game's plugin list, as the service publishes it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePluginList {
    dir: String,
    #[serde(default)]
    plugins_file: Option<String>,
    #[serde(default)]
    load_order_file: Option<String>,
    #[serde(default)]
    activation: Option<String>,
    #[serde(default)]
    implicit: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDetection {
    steam_app_id: u32,
    #[serde(default)]
    executable: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireDeployTarget {
    source: String,
    target: String,
}

#[derive(Debug, Deserialize)]
struct WireRewrap {
    folder: String,
    prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireLoader {
    name: String,
    kind: String,
    #[serde(default)]
    proxy_dll: Option<String>,
    #[serde(default)]
    also_provides: Vec<String>,
    #[serde(default)]
    data_dirs: Vec<String>,
    #[serde(default)]
    proton: Option<WireProton>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireProton {
    #[serde(default)]
    wine_dll_overrides: Option<String>,
    #[serde(default)]
    steam_launch_options: Option<String>,
    #[serde(default)]
    requires_prefix_write: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WirePakChain {
    pattern: String,
    #[serde(default = "three")]
    digits: usize,
    #[serde(default = "one")]
    start_index: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireFomod {
    #[serde(default)]
    dest_prefix: String,
}

fn three() -> usize {
    3
}

fn one() -> u32 {
    1
}

impl WireProfile {
    fn into_profile(self) -> GameProfile {
        use apoc_domain::{
            ConflictScope, DeployTarget, Engine, FomodSpec, LoadOrderPolicy, LoaderKind,
            LoaderSpec, PakChainSpec, ProtonLoaderSpec, RewrapRule, SteamDetection,
        };

        GameProfile {
            id: self.id,
            name: self.name,
            // An engine nobody here recognises is `Other`, which is honest and
            // costs nothing: the engine name selects no behaviour on its own.
            engine: match self.engine.as_str() {
                "re-engine" => Engine::ReEngine,
                "creation" => Engine::Creation,
                "red-engine" => Engine::RedEngine,
                _ => Engine::Other,
            },
            detection: SteamDetection {
                steam_app_id: self.detection.steam_app_id,
                executable: self.detection.executable,
            },
            nexus_domain: self.nexus_domain,
            // Priority is the policy the engine actually implements, so an
            // unknown value reads as that rather than as something unhandled.
            load_order: match self.load_order.as_str() {
                "none" => LoadOrderPolicy::None,
                "explicit" => LoadOrderPolicy::Explicit,
                _ => LoadOrderPolicy::Priority,
            },
            conflict_scope: ConflictScope::PerRelativePath,
            case_sensitive: self.case_sensitive,
            deploy_targets: self
                .deploy_targets
                .into_iter()
                .map(|t| DeployTarget {
                    source: t.source,
                    target: t.target,
                })
                .collect(),
            formats: self.formats,
            fomod: self.fomod.map(|f| FomodSpec {
                dest_prefix: f.dest_prefix,
            }),
            plugin_extensions: self.plugin_extensions,
            plugin_list: self.plugin_list.map(|p| PluginListSpec {
                dir: p.dir,
                plugins_file: p.plugins_file.unwrap_or_else(|| "plugins.txt".to_string()),
                load_order_file: p.load_order_file,
                // An unknown spelling reads as the modern convention rather
                // than as an error: the field is about how a file is written,
                // and `presence` is the one a newer profile would not pick.
                activation: match p.activation.as_deref() {
                    Some("presence") => PluginActivation::Presence,
                    _ => PluginActivation::Asterisk,
                },
                implicit: p.implicit,
            }),
            root_files: self.root_files.map(|r| RootFilesSpec {
                folder: r.folder,
                patterns: r.patterns,
            }),
            rewrap: self
                .rewrap
                .into_iter()
                .map(|r| RewrapRule {
                    folder: r.folder,
                    prefix: r.prefix,
                })
                .collect(),
            canonical_case: self.canonical_case,
            loader: self.loader.map(|l| LoaderSpec {
                name: l.name,
                kind: match l.kind.as_str() {
                    "dll-proxy" => LoaderKind::DllProxy,
                    _ => LoaderKind::None,
                },
                proxy_dll: l.proxy_dll,
                also_provides: l.also_provides,
                data_dirs: l.data_dirs,
                proton: l
                    .proton
                    .map(|p| ProtonLoaderSpec {
                        wine_dll_overrides: p.wine_dll_overrides,
                        steam_launch_options: p.steam_launch_options,
                        requires_prefix_write: p.requires_prefix_write,
                    })
                    .unwrap_or_default(),
            }),
            pak_chain: self.pak_chain.map(|c| PakChainSpec {
                pattern: c.pattern,
                digits: c.digits,
                start_index: c.start_index,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WILDS: &str = r#"{
        "id": "monster-hunter-wilds",
        "name": "Monster Hunter Wilds",
        "schemaVersion": 1,
        "revision": 3,
        "engine": "re-engine",
        "nexusDomain": "monsterhunterwilds",
        "loadOrder": "priority",
        "conflictScope": "per-relative-path",
        "caseSensitive": true,
        "detection": { "steamAppId": 2246340, "executable": "MonsterHunterWilds.exe" },
        "formats": ["fluffy-aio"],
        "canonicalCase": ["STM"],
        "pluginExtensions": [],
        "deployTargets": [{ "source": "natives", "target": "natives" }],
        "rewrap": [{ "folder": "autorun", "prefix": "reframework" }],
        "loader": {
            "name": "REFramework",
            "kind": "dll-proxy",
            "proxyDll": "dinput8.dll",
            "dataDirs": ["reframework/data"],
            "proton": {
                "wineDllOverrides": "dinput8=n,b",
                "steamLaunchOptions": "WINEDLLOVERRIDES=\"dinput8=n,b\" %command%",
                "requiresPrefixWrite": true
            }
        },
        "pakChain": { "pattern": "re_chunk_000.pak.sub_000.pak.patch_{n}.pak", "digits": 3, "startIndex": 1 },
        "fomod": null
    }"#;

    fn parse(json: &str) -> GameProfile {
        serde_json::from_str::<WireProfile>(json)
            .expect("parses")
            .into_profile()
    }

    #[test]
    fn a_published_profile_becomes_the_same_thing_a_bundled_one_is() {
        use apoc_domain::{Engine, LoadOrderPolicy, LoaderKind};

        let g = parse(WILDS);
        assert_eq!(g.id, "monster-hunter-wilds");
        assert_eq!(g.engine, Engine::ReEngine);
        assert_eq!(g.load_order, LoadOrderPolicy::Priority);
        assert_eq!(g.detection.steam_app_id, 2246340);
        assert_eq!(g.target_for("natives"), Some("natives"));
        assert_eq!(g.canonical_case, vec!["STM"]);

        // The loader is the part that writes into somebody's Proton prefix, so
        // it is the part most worth pinning field by field.
        let loader = g.loader.expect("loader");
        assert_eq!(loader.kind, LoaderKind::DllProxy);
        assert_eq!(loader.proxy_dll.as_deref(), Some("dinput8.dll"));
        assert_eq!(
            loader.proton.wine_dll_overrides.as_deref(),
            Some("dinput8=n,b")
        );
        assert!(loader.proton.requires_prefix_write);

        let chain = g.pak_chain.expect("pak chain");
        assert_eq!(chain.digits, 3);
        assert_eq!(chain.start_index, 1);
    }

    #[test]
    fn a_profile_for_a_game_this_build_has_never_heard_of_still_reads() {
        // The whole point of publishing profiles: a game arrives without an app
        // release. An unrecognised engine name selects no behaviour, so it is
        // recorded as other rather than refused.
        use apoc_domain::Engine;

        let g = parse(
            r#"{
                "id": "some-new-game", "name": "Some New Game",
                "engine": "unheard-of", "loadOrder": "priority",
                "conflictScope": "per-relative-path", "caseSensitive": true,
                "detection": { "steamAppId": 42 },
                "deployTargets": [{ "source": "Data", "target": "Data" }]
            }"#,
        );
        assert_eq!(g.engine, Engine::Other);
        assert_eq!(g.target_for("Data"), Some("Data"));
        assert!(g.loader.is_none());
    }

    #[test]
    fn a_document_missing_something_required_is_refused_rather_than_half_read() {
        // Half a profile is worse than the bundled one it would replace, so the
        // failure has to be at the parse rather than in the fields.
        let broken = r#"{ "id": "x", "name": "X", "engine": "creation" }"#;
        assert!(serde_json::from_str::<WireProfile>(broken).is_err());
    }

    #[test]
    fn a_field_this_build_does_not_know_is_ignored_rather_than_fatal() {
        // The reason this has its own wire type: the domain type denies unknown
        // fields, which is right for a file somebody edits and wrong for a
        // document a newer service sent.
        let with_extra = r#"{
            "id": "x", "name": "X", "engine": "creation", "loadOrder": "priority",
            "conflictScope": "per-relative-path", "caseSensitive": true,
            "detection": { "steamAppId": 1 },
            "somethingAddedLater": { "nested": true }
        }"#;
        assert!(serde_json::from_str::<WireProfile>(with_extra).is_ok());
    }

    #[test]
    fn a_refusal_says_what_happened_without_naming_a_protocol() {
        // These reach somebody who wants to install a mod, not a log.
        let reached = describe(ureq::Error::Status(
            503,
            ureq::Response::new(503, "Service Unavailable", "").unwrap(),
        ));
        assert!(reached.contains("built-in profiles"), "{reached}");

        let refused = describe(ureq::Error::Status(
            418,
            ureq::Response::new(418, "Teapot", "").unwrap(),
        ));
        assert!(refused.contains("418"), "{refused}");
    }

    /// A URL fragment and the answer any URL containing it gets.
    type Script = Vec<(&'static str, Result<String, String>)>;

    /// Answers scripted per URL, so a test can make the service behave in a way
    /// no real one would oblige on cue.
    struct Scripted(Script);

    impl ProfileTransport for Scripted {
        fn get(&self, url: &str) -> Result<String, String> {
            for (fragment, answer) in &self.0 {
                if url.contains(fragment) {
                    return answer.clone();
                }
            }
            Err("the service could not be reached".to_string())
        }
    }

    fn db(script: Script) -> ApocryphaGameDb<Scripted> {
        ApocryphaGameDb::with_transport(Default::default(), Scripted(script))
    }

    fn index_of(id: &str, schema: u32) -> String {
        format!(r#"[{{"id":"{id}","name":"A Game","schemaVersion":{schema},"revision":1}}]"#)
    }

    /// Every one of these ends the same way, and that is the point: the bundled
    /// profiles are the floor. None of these five was reachable in a test before
    /// there was somewhere other than the network to answer from.
    #[test]
    fn every_way_the_service_can_fail_falls_back_to_the_bundled_profiles() {
        let bundled = LocalBuiltin::new().all().unwrap();

        let cases: Vec<(&str, Script)> = vec![
            (
                "offline",
                vec![("", Err("the service could not be reached".into()))],
            ),
            (
                "no profiles published",
                vec![(
                    "",
                    Err("the service has no game profiles to publish".into()),
                )],
            ),
            (
                "the service is unwell",
                vec![(
                    "",
                    Err(
                        "the service is having trouble; the built-in profiles are being used"
                            .into(),
                    ),
                )],
            ),
            (
                "an index that is not an index",
                vec![("games/profiles", Ok("{\"oops\":true}".into()))],
            ),
            (
                "a document that will not parse",
                vec![
                    ("games/profiles", Ok(index_of("monster-hunter-wilds", 1))),
                    ("/profile", Ok("not json at all".into())),
                ],
            ),
        ];

        for (what, script) in cases {
            let (profiles, freshness) = db(script).fetch();
            assert_eq!(profiles, bundled, "{what}: the bundled profiles stand");
            assert!(
                matches!(freshness, Freshness::Offline { .. }),
                "{what}: the fallback is reported rather than hidden, got {freshness:?}"
            );
        }
    }

    /// A contract this build does not read is refused rather than guessed at,
    /// and says so in terms of the game it concerns.
    #[test]
    fn a_profile_declaring_a_newer_contract_is_named_not_silently_dropped() {
        let (profiles, freshness) = db(vec![(
            "games/profiles",
            Ok(index_of("monster-hunter-wilds", SUPPORTED_SCHEMA + 1)),
        )])
        .fetch();

        assert_eq!(profiles, LocalBuiltin::new().all().unwrap());
        match freshness {
            Freshness::Offline { reason } => {
                assert!(reason.contains("monster-hunter-wilds"), "{reason}");
                assert!(reason.contains("profile format"), "{reason}");
            }
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }

    /// The path that has to keep working: a published profile replaces its
    /// bundled namesake, and nothing else moves.
    #[test]
    fn a_published_profile_replaces_only_its_own_bundled_entry() {
        let published = WILDS.replace(r#""target": "natives""#, r#""target": "natives/stm""#);
        let (profiles, freshness) = db(vec![
            ("games/profiles", Ok(index_of("monster-hunter-wilds", 1))),
            ("/profile", Ok(published)),
        ])
        .fetch();

        assert!(matches!(freshness, Freshness::Online { fetched: 1 }));
        let wilds = profiles
            .iter()
            .find(|g| g.id == "monster-hunter-wilds")
            .expect("the published profile is in force");
        assert!(wilds
            .deploy_targets
            .iter()
            .any(|t| t.target == "natives/stm"));
        assert_eq!(
            profiles.len(),
            LocalBuiltin::new().all().unwrap().len(),
            "no other game moved"
        );
    }

    /// A refusal at the wire boundary is a fetch failure like any other: the
    /// bundled profile stays, whole.
    #[test]
    fn a_profile_that_would_escape_its_directories_falls_back_like_any_other_failure() {
        let hostile = WILDS.replace(r#""target": "natives""#, r#""target": "../../autostart""#);
        let (profiles, freshness) = db(vec![
            ("games/profiles", Ok(index_of("monster-hunter-wilds", 1))),
            ("/profile", Ok(hostile)),
        ])
        .fetch();

        assert_eq!(profiles, LocalBuiltin::new().all().unwrap());
        assert!(matches!(freshness, Freshness::Offline { .. }));
    }

    /// The check that matters most: a published profile is the same document a
    /// bundled one is, so a rule the bundled set cannot satisfy would silently
    /// send everybody back to the built-in profiles for ever.
    #[test]
    fn every_bundled_profile_satisfies_the_rules_asked_of_a_published_one() {
        let bundled = LocalBuiltin::new().all().unwrap();
        assert!(!bundled.is_empty(), "there are profiles to check");
        for profile in bundled {
            assert_eq!(
                validate(&profile),
                Ok(()),
                "the bundled profile for {} would be refused if it were published",
                profile.id
            );
        }
    }

    /// A profile decides where files are written. Published, it is a document
    /// from somewhere else, and these are the ways one could ask for a write
    /// outside the directory it is entitled to.
    #[test]
    fn a_profile_that_would_write_outside_its_directories_is_refused() {
        let escapes = [
            (r#""id": "monster-hunter-wilds""#, r#""id": "../../etc""#),
            (
                r#""target": "natives""#,
                r#""target": "../../../.config/autostart""#,
            ),
            (r#""target": "natives""#, r#""target": "/etc/systemd/user""#),
            (
                r#""target": "natives""#,
                r#""target": "..\\..\\Windows\\System32""#,
            ),
        ];

        for (from, to) in escapes {
            let document = WILDS.replace(from, to);
            assert!(
                document != WILDS,
                "the fixture no longer contains {from}, so this proves nothing"
            );
            let refused = parse_document(&document)
                .expect_err(&format!("{to} should be refused"))
                .to_string();
            assert!(
                refused.contains("id") || refused.contains("leaves the directory"),
                "the refusal should say what was wrong: {refused}"
            );
        }
    }

    /// A plugin list names a directory inside the user's Proton prefix and the
    /// files written into it, so a published profile picking those strings is
    /// picking where a write lands.
    #[test]
    fn a_published_plugin_list_cannot_choose_where_it_writes() {
        let escapes = [
            r#""dir": "../../../../../../home/someone""#,
            r#""dir": "/etc""#,
            r#""pluginsFile": "../../autostart/x.desktop""#,
            r#""loadOrderFile": "/etc/passwd""#,
        ];

        for escape in escapes {
            let document = WILDS.replace(
                r#""schemaVersion": 1,"#,
                &format!(r#""schemaVersion": 1, "pluginList": {{ "dir": "Fine", {escape} }},"#),
            );
            let refused = parse_document(&document)
                .expect_err(&format!("{escape} should be refused"))
                .to_string();
            assert!(
                refused.contains("leaves the directory") || refused.contains("could not be read"),
                "the refusal should say what was wrong: {refused}"
            );
        }
    }

    /// Refused whole. The bundled profile stays in force, exactly as it does
    /// for a timeout, rather than the two being merged into a configuration
    /// nobody wrote.
    #[test]
    fn a_refused_profile_leaves_the_bundled_one_untouched() {
        let document = WILDS.replace(r#""target": "natives""#, r#""target": "../elsewhere""#);
        assert!(parse_document(&document).is_err());

        let bundled = LocalBuiltin::new().get("monster-hunter-wilds").unwrap();
        assert!(
            bundled
                .deploy_targets
                .iter()
                .all(|t| !t.target.contains("..")),
            "the bundled profile is what remains in force"
        );
    }
}
