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

// Nothing calls the parser yet: `normalize` still refuses a detected FOMOD, and
// the lowering step that will call this arrives next. Kept as its own commit so
// the parser can be read and reviewed on its own, against the format, without a
// reviewer also holding the archive resolution in their head.
#[allow(dead_code)]
pub(crate) mod xml;
