//! The mod engine: read a mod archive and normalize it into the canonical
//! [`apoc_domain::ModBundle`] that drives the install wizard and, later,
//! deployment.
//!
//! Entry point: [`analyze_archive`]. It detects the installer model
//! (Fluffy AIO / single Fluffy mod / flat `natives/` dump / REFramework-only)
//! and returns a uniform bundle regardless of source shape.

mod archive;
pub mod carry;
mod error;
mod extract;
mod modinfo;
mod naming;
mod normalize;
pub mod plan;
mod rules;

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
    Ok(normalize::normalize(index, &stem_of(path), sha, rules))
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
    Ok(normalize::normalize(index, &stem_of(path), None, rules))
}

/// Analyze without hashing, using default rules.
pub fn analyze_archive_no_hash(path: &Path) -> Result<ModBundle> {
    analyze_archive_no_hash_with(path, &GameRules::default())
}
