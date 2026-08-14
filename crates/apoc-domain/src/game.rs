//! Declarative game definitions. A game is **data**, never hardcoded logic.
//!
//! Everything the engines need to detect, install into, deploy to, and load a
//! game is expressed here so that adding a new game is (ideally) adding one
//! `GameProfile` document: no changes to `apoc-modengine`, `apoc-deploy`, etc.

use serde::{Deserialize, Serialize};

/// The engine a game is built on. Drives default format detectors and loader
/// expectations, but does not itself contain behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    /// Capcom RE Engine (Monster Hunter Wilds, Dragon's Dogma 2, ...).
    ReEngine,
    /// Bethesda Creation Engine (Skyrim SE, Fallout 4).
    Creation,
    /// CD Projekt RED engine (Cyberpunk 2077).
    RedEngine,
    /// Anything not yet modeled; forces manual/loose handling.
    Other,
}

/// Whether load order matters for this game, and how it is expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadOrderPolicy {
    /// Order is irrelevant (rare); deployment winner is arbitrary-but-stable.
    None,
    /// Last-writer-wins by a user-orderable priority (RE Engine loose files).
    Priority,
    /// Explicit, named ordering with rules (Creation Engine plugin list).
    Explicit,
}

/// How file overlaps are scoped when computing conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictScope {
    /// Two mods conflict iff they write the identical relative path.
    PerRelativePath,
}

/// How a game is located on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteamDetection {
    /// Steam application id, e.g. Monster Hunter Wilds = 2246340.
    pub steam_app_id: u32,
    /// Executable name to sanity-check an install directory (best-effort).
    #[serde(default)]
    pub executable: Option<String>,
}

/// A payload root inside a normalized mod, mapped to where it lands relative to
/// the game install directory. `source` is the archive-side top-level directory
/// (`natives`, `reframework`); `target` is the game-relative destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeployTarget {
    pub source: String,
    pub target: String,
}

/// The kind of mod loader a game needs, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoaderKind {
    /// No loader; the game reads loose files natively.
    None,
    /// A DLL proxy loader (e.g. REFramework via `dinput8.dll`).
    DllProxy,
}

/// Proton-specific loader provisioning knobs. These are the crux of Linux
/// support: getting a Windows DLL loader to run under Proton.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtonLoaderSpec {
    /// Value for `[Software\Wine\DllOverrides]`, e.g. `dinput8=n,b`.
    #[serde(default)]
    pub wine_dll_overrides: Option<String>,
    /// Full Steam launch-option string, e.g. `WINEDLLOVERRIDES="dinput8=n,b" %command%`.
    #[serde(default)]
    pub steam_launch_options: Option<String>,
    /// Whether the prefix must be writable (registry override) to work.
    #[serde(default)]
    pub requires_prefix_write: bool,
}

/// A loader specification: what proxy DLL to place and how to make Proton load it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoaderSpec {
    pub name: String,
    pub kind: LoaderKind,
    /// The proxy DLL that must exist for the loader to run (real copy, never
    /// linked). Game-relative, so it may name a subdirectory: REFramework wants
    /// `dinput8.dll` in the game root, RED4ext wants `bin/x64/winmm.dll`.
    #[serde(default)]
    pub proxy_dll: Option<String>,
    /// Game-relative directories the loader reads its own data from.
    #[serde(default)]
    pub data_dirs: Vec<String>,
    #[serde(default)]
    pub proton: ProtonLoaderSpec,
}

impl LoaderSpec {
    /// The registry key name for the proxy DLL: its file stem, never its path.
    /// Wine's `DllOverrides` are keyed by module name, so `bin/x64/winmm.dll`
    /// registers as `winmm`.
    pub fn proxy_dll_stem(&self) -> Option<&str> {
        let dll = self.proxy_dll.as_deref()?;
        let name = dll.rsplit('/').next().unwrap_or(dll);
        Some(name.strip_suffix(".dll").unwrap_or(name))
    }

    /// The DLL overrides this loader needs, as `(module, value)` pairs.
    ///
    /// `WINEDLLOVERRIDES` is a semicolon-separated list, and a game can need
    /// more than one entry: Cyberpunk wants `winmm` for RED4ext and `version`
    /// for Cyber Engine Tweaks, which are separate proxies in the same folder.
    pub fn dll_overrides(&self) -> Vec<(String, String)> {
        let raw = match self.proton.wine_dll_overrides.as_deref() {
            Some(s) => s,
            None => return Vec::new(),
        };
        raw.split(';')
            .filter_map(|entry| {
                let (name, value) = entry.trim().split_once('=')?;
                let name = name.trim();
                let value = value.trim();
                if name.is_empty() || value.is_empty() {
                    return None;
                }
                Some((name.to_string(), value.to_string()))
            })
            .collect()
    }
}

/// How standalone `.pak` mods are slotted into an RE Engine patch chain.
///
/// RE Engine loads a base archive plus numbered patch archives; a mod PAK is
/// only seen by the game if its filename joins that chain. Monster Hunter Wilds
/// uses the "sub" scheme, e.g. `re_chunk_000.pak.sub_000.pak.patch_003.pak`.
/// The index is assigned at deploy time, above whatever already exists on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PakChainSpec {
    /// Filename template; `{n}` is replaced by the zero-padded patch index.
    pub pattern: String,
    /// Zero-padding width for `{n}`.
    #[serde(default = "default_pak_digits")]
    pub digits: usize,
    /// Lowest index this manager will assign.
    #[serde(default = "default_pak_start")]
    pub start_index: u32,
}

fn default_pak_digits() -> usize {
    3
}

fn default_pak_start() -> u32 {
    1
}

impl PakChainSpec {
    /// Render the chain filename for a given index.
    pub fn filename(&self, index: u32) -> String {
        self.pattern
            .replace("{n}", &format!("{index:0width$}", width = self.digits))
    }

    /// Extract the patch index from a filename produced by this spec.
    pub fn index_of(&self, filename: &str) -> Option<u32> {
        let (prefix, suffix) = self.pattern.split_once("{n}")?;
        let rest = filename.strip_prefix(prefix)?.strip_suffix(suffix)?;
        rest.parse().ok()
    }
}

/// A folder that mod authors sometimes ship at the archive root because they
/// zipped the *inside* of a payload directory. `autorun/` at the root really
/// means `reframework/autorun/`, and without this the mod imports as zero files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewrapRule {
    /// Folder name found at the archive root.
    pub folder: String,
    /// Prefix to restore above it.
    pub prefix: String,
}

/// Game-specific FOMOD behaviour.
///
/// A FOMOD declares where each file goes, but it declares it in the terms its
/// own community uses. A Skyrim installer writes `destination="meshes/x.nif"`
/// and means `Data/meshes/x.nif`, because on that game every mod lives under
/// `Data` and nobody writes it out. An RE Engine mod means what it says.
///
/// That difference belongs to the game, so it is declared here rather than
/// discovered by looking at which game is loaded — which is the rule the whole
/// gamedef layer exists to keep.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FomodSpec {
    /// Prefixed to every destination a FOMOD declares. Empty when the game's
    /// installers already write game-root-relative paths.
    #[serde(default)]
    pub dest_prefix: String,
}

/// A complete, declarative game definition. This is the plugin unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProfile {
    /// Stable slug, e.g. `monster-hunter-wilds`.
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub detection: SteamDetection,
    /// Nexus Mods game domain, e.g. `monsterhunterwilds`. This is the segment
    /// an `nxm://` link carries, so it is how an incoming download is matched
    /// to the game it belongs to rather than to whichever game is on screen.
    #[serde(default)]
    pub nexus_domain: Option<String>,
    pub load_order: LoadOrderPolicy,
    pub conflict_scope: ConflictScope,
    /// Linux ext4 is case-sensitive; RE Engine paths must be preserved verbatim.
    #[serde(default = "default_true")]
    pub case_sensitive: bool,
    /// Where each payload root lands relative to the game install directory.
    pub deploy_targets: Vec<DeployTarget>,
    /// Archive shapes this game's mods come in (`fluffy-aio`, `loose-roots`,
    /// ...).
    ///
    /// Mostly descriptive: detection derives what it needs from the payload
    /// roots, the rewrap rules and the loader. The exception is `fomod`, which
    /// no amount of looking at an archive's shape can imply — a FOMOD announces
    /// itself in a manifest, and whether this game's mods are expected to use
    /// one is a statement only the profile can make.
    #[serde(default)]
    pub formats: Vec<String>,
    /// How FOMOD installers behave for this game, when its mods ship them.
    #[serde(default)]
    pub fomod: Option<FomodSpec>,
    /// File extensions that carry load order rather than content, such as
    /// Creation Engine's `esp`, `esm` and `esl`.
    ///
    /// Apocrypha deploys these files and does **not** enable or order them: a
    /// Creation Engine game reads its own plugin list, which nothing here
    /// writes. Declaring them is what lets the manager say so, instead of
    /// leaving someone to conclude a mod failed to install when it was
    /// installed and simply never switched on.
    ///
    /// Empty for engines with no such concept, which is every game shipped so
    /// far except Skyrim.
    #[serde(default)]
    pub plugin_extensions: Vec<String>,
    /// Root folders to re-wrap under a payload prefix when an archive is packed
    /// from the inside out.
    #[serde(default)]
    pub rewrap: Vec<RewrapRule>,
    /// Path components whose casing the engine is strict about. Enforced so a
    /// Windows-authored archive lands in one tree on a case-sensitive filesystem.
    #[serde(default)]
    pub canonical_case: Vec<String>,
    /// Loader requirement, if any.
    #[serde(default)]
    pub loader: Option<LoaderSpec>,
    /// How standalone `.pak` mods join the engine's patch chain, if supported.
    #[serde(default)]
    pub pak_chain: Option<PakChainSpec>,
}

impl GameProfile {
    /// Resolve the game-relative destination for a source top-level dir, if this
    /// profile deploys it. E.g. `"natives"` -> `Some("natives")`.
    pub fn target_for(&self, source_root: &str) -> Option<&str> {
        self.deploy_targets
            .iter()
            .find(|t| t.source == source_root)
            .map(|t| t.target.as_str())
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loader(dll: &str, overrides: Option<&str>) -> LoaderSpec {
        LoaderSpec {
            name: "L".into(),
            kind: LoaderKind::DllProxy,
            proxy_dll: Some(dll.into()),
            data_dirs: vec![],
            proton: ProtonLoaderSpec {
                wine_dll_overrides: overrides.map(str::to_string),
                steam_launch_options: None,
                requires_prefix_write: true,
            },
        }
    }

    #[test]
    fn the_registry_key_is_the_module_name_not_the_path() {
        // Wine keys DllOverrides by module, so a loader that lives in a
        // subdirectory must still register as a bare name.
        assert_eq!(
            loader("dinput8.dll", None).proxy_dll_stem(),
            Some("dinput8")
        );
        assert_eq!(
            loader("bin/x64/winmm.dll", None).proxy_dll_stem(),
            Some("winmm")
        );
    }

    #[test]
    fn a_game_can_need_more_than_one_override() {
        let l = loader("bin/x64/winmm.dll", Some("winmm=n,b;version=n,b"));
        assert_eq!(
            l.dll_overrides(),
            vec![
                ("winmm".to_string(), "n,b".to_string()),
                ("version".to_string(), "n,b".to_string()),
            ]
        );
    }

    #[test]
    fn malformed_override_entries_are_dropped_not_guessed() {
        let l = loader("x.dll", Some("winmm=n,b; ; =n,b; version="));
        assert_eq!(
            l.dll_overrides(),
            vec![("winmm".to_string(), "n,b".to_string())]
        );
        assert!(loader("x.dll", None).dll_overrides().is_empty());
    }
}
