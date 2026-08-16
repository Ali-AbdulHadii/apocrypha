//! The mod engine: read a mod archive and normalize it into the canonical
//! [`apoc_domain::ModBundle`] that drives the install wizard and, later,
//! deployment.
//!
//! Entry point: [`analyze_archive`]. It detects the installer model
//! (Fluffy AIO / single Fluffy mod / flat `natives/` dump / REFramework-only /
//! FOMOD) and returns a uniform bundle regardless of source shape.

mod archive;
pub mod carry;
mod error;
mod extract;
pub mod fomod;
mod modinfo;
mod naming;
mod normalize;
pub mod plan;
pub mod plugins;
mod rules;

// Indexing is exposed for tests and tooling that need to see what an archive
// holds without committing to a bundle. The rest of `archive` stays internal:
// the three container formats are not interchangeable, and callers should keep
// going through the one shape that hides that.
pub use archive::{read_index, ArchiveIndex, FomodSource};
pub use carry::{carry_selection, CarriedSelection};
pub use error::{ModEngineError, Result};
pub use extract::{
    preview_rel_path, read_archive_entry, stage_bundle, staged_preview, StageReport, PREVIEW_DIR,
};
pub use modinfo::Modinfo;
pub use plan::{
    choose_exclusive, combine as combine_plans,
    combine_with_overrides as combine_plans_with_overrides, default_selection,
    plan as plan_deployment, recommended_selection, toggle, ConflictOverrides, ModPlan,
};
pub use rules::GameRules;

use apoc_domain::ModBundle;
use std::path::Path;

/// Say plainly when a mod ships files whose load order this application does
/// not manage.
///
/// A Creation Engine game reads its own plugin list, and nothing here writes
/// it. So a Skyrim mod installs, its files land exactly where they belong, and
/// the game ignores them until the plugins are enabled somewhere else. That is
/// the safe failure — the alternative is writing a user's load order without
/// being asked — but it is invisible, and invisible is what makes it read as
/// the manager having failed.
///
/// Driven entirely by what the game profile declares, so a game gets this by
/// saying which extensions carry load order, never by being named in code.
/// Returns nothing at all for an engine with no such concept.
pub fn unmanaged_plugin_notice(bundle: &ModBundle, rules: &GameRules) -> Option<String> {
    let mut names: Vec<&str> = bundle
        .options()
        .flat_map(|o| o.payload.iter())
        .filter(|f| rules.is_plugin_file(&f.game_rel_path))
        .filter_map(|f| f.game_rel_path.rsplit('/').next())
        .collect();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return None;
    }

    let listed = if names.len() > 3 {
        format!("{}, and {} more", names[..3].join(", "), names.len() - 3)
    } else {
        names.join(", ")
    };
    Some(format!(
        "This mod includes plugin files ({listed}). Apocrypha installs them but does not manage \
         the game's plugin list yet, so enable and sort them in your usual tool afterwards or the \
         game will not load them."
    ))
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mod")
        .to_string()
}

/// Analyze a mod archive at `path` and return its normalized [`ModBundle`],
/// including the archive's SHA-256 (used later for update detection and dedupe).
///
/// `rules` decide what counts as deployable content; build them from the active
/// game with [`GameRules::from_profile`] so loader DLLs and engine-specific
/// payload roots are recognized.
pub fn analyze_archive_with(path: &Path, rules: &GameRules) -> Result<ModBundle> {
    let index = archive::read_index(path)?;
    if index.entries.is_empty() {
        return Err(ModEngineError::Empty);
    }
    let sha = archive::hash_archive(path).ok();
    normalize::normalize(index, &stem_of(path), sha, rules)
}

/// Analyze with default (RE Engine) rules.
pub fn analyze_archive(path: &Path) -> Result<ModBundle> {
    analyze_archive_with(path, &GameRules::default())
}

/// Analyze without hashing the archive (faster; used where the hash is not needed).
pub fn analyze_archive_no_hash_with(path: &Path, rules: &GameRules) -> Result<ModBundle> {
    let index = archive::read_index(path)?;
    if index.entries.is_empty() {
        return Err(ModEngineError::Empty);
    }
    normalize::normalize(index, &stem_of(path), None, rules)
}

/// Analyze without hashing, using default rules.
pub fn analyze_archive_no_hash(path: &Path) -> Result<ModBundle> {
    analyze_archive_no_hash_with(path, &GameRules::default())
}

/// The distinct top-level directory names an archive contains, in first-seen
/// order, after any redundant wrapper directory has been stripped.
///
/// This exists for one purpose: explaining a mod that analysed to nothing. When
/// no file matched a payload root, the bundle is empty and cannot say why, so
/// the roots the archive actually has are the missing half of the answer —
/// "this archive has `Data/` and `Meshes/`, this game reads `archive/`, `r6/`,
/// …" is a diagnosis, where "0 files" is only a symptom. Deliberately narrower
/// than exposing the archive index itself, which is an internal shape.
pub fn archive_roots(path: &Path) -> Result<Vec<String>> {
    let index = archive::read_index(path)?;
    let mut roots: Vec<String> = Vec::new();
    for entry in &index.entries {
        let root = entry.path.split('/').next().unwrap_or_default();
        if root.is_empty() {
            continue;
        }
        // A file at the archive root is itself a root, and naming it matters:
        // a bare `dinput8.dll` is a loader release, not an empty archive.
        if !roots.iter().any(|r| r == root) {
            roots.push(root.to_string());
        }
    }
    Ok(roots)
}
