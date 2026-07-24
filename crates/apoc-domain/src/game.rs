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
    /// The proxy DLL filename that must exist in the game root (real copy, never linked).
    #[serde(default)]
    pub proxy_dll: Option<String>,
    /// Game-relative directories the loader reads its own data from.
    #[serde(default)]
    pub data_dirs: Vec<String>,
    #[serde(default)]
    pub proton: ProtonLoaderSpec,
}

/// How standalone `.pak` mods are slotted into an RE Engine patch chain.
///
/// RE Engine loads a base archive plus numbered patch archives; a mod PAK is
/// only seen by the game if its filename joins that chain. Monster Hunter Wilds
/// uses the "sub" scheme, e.g. `re_chunk_000.pak.sub_000.pak.patch_003.pak`.
/// The index is assigned at deploy time, above whatever already exists on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct RewrapRule {
    /// Folder name found at the archive root.
    pub folder: String,
    /// Prefix to restore above it.
    pub prefix: String,
}

/// A complete, declarative game definition. This is the plugin unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProfile {
    /// Stable slug, e.g. `monster-hunter-wilds`.
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub detection: SteamDetection,
    pub load_order: LoadOrderPolicy,
    pub conflict_scope: ConflictScope,
    /// Linux ext4 is case-sensitive; RE Engine paths must be preserved verbatim.
    #[serde(default = "default_true")]
    pub case_sensitive: bool,
    /// Where each payload root lands relative to the game install directory.
    pub deploy_targets: Vec<DeployTarget>,
    /// Format detector ids this game accepts (`fluffy-aio`, `loose-natives`, ...).
    #[serde(default)]
    pub formats: Vec<String>,
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
