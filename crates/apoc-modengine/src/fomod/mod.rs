//! FOMOD: reading a conditional installer and turning it into something the
//! rest of the engine already understands.
//!
//! The split here follows the one property that makes the format tractable:
//! **which files an option installs is fixed, while whether it applies is not.**
//! A plugin's `<files>` are known as soon as the manifest is read, so they are
//! resolved against the archive once, at analyze time. Only visibility and
//! plugin type depend on the answers a user gives, and those are decided over
//! option ids alone, with no archive in hand.
//!
//! - [`xml`] turns `ModuleConfig.xml` into a [`apoc_domain::fomod::FomodModule`].
//! - `lower` (next) resolves that against an archive into a `ModBundle`.
//! - `eval` (after that) answers which options apply, given a set of choices.

pub mod eval;
pub(crate) mod lower;
pub(crate) mod xml;

use crate::archive::ArchiveIndex;
use crate::error::Result;
use crate::rules::GameRules;
use apoc_domain::ModBundle;

/// Read an archive's FOMOD manifest and resolve it into a bundle.
pub(crate) fn analyze(
    index: &ArchiveIndex,
    archive_stem: &str,
    sha: Option<String>,
    rules: &GameRules,
) -> Result<ModBundle> {
    let source = index
        .fomod
        .as_ref()
        .expect("detection only reports Fomod when a manifest was found");
    let module = xml::parse_module_config(&source.config)?;
    lower::lower(&module, index, archive_stem, sha, rules)
}
