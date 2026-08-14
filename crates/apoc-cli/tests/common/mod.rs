//! Finding the real mod archives the acceptance tests run against.
//!
//! Some tests here are driven by archives people actually downloaded rather than
//! by ones the test builds. That is the whole point of them: a synthesized
//! archive proves the engine agrees with the shape the test author imagined,
//! while a real one proves it agrees with what an author shipped. The cost is
//! that the archives are other people's work and run to tens of megabytes, so
//! they are never committed and every test using them skips when they are
//! absent.
//!
//! Skipping quietly has a failure mode of its own, and this module exists
//! because the repository hit it: the sample archive was tidied into
//! `scratch/sample-archives/`, one directory below where the finder looked, and
//! the Phase 1 acceptance test went on passing without running a single
//! assertion. So the search covers every location the archives have plausibly
//! been put, and a test that skips says which directories it looked in.

use std::path::{Path, PathBuf};

/// Directories that may hold fixture archives, nearest first.
///
/// Both roots are searched because the repository has lived in both shapes: a
/// `scratch/` inside it, and a `scratch/` beside it in the planning workspace
/// that also holds the sibling web project.
fn roots() -> Vec<PathBuf> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = crate_dir.join("..").join("..");
    let workspace_root = repo_root.join("..");
    vec![repo_root, workspace_root]
}

/// Every directory a fixture of this kind might be in, in search order.
pub fn fixture_dirs(env_var: &str, subdir: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(from_env) = std::env::var_os(env_var) {
        dirs.push(PathBuf::from(from_env));
    }
    for root in roots() {
        dirs.push(root.join("scratch").join(subdir));
        // Historically the archives sat loose at a root rather than under
        // `scratch/`. Kept so an older working copy still runs these tests
        // rather than skipping them without saying why.
        dirs.push(root);
    }
    dirs
}

/// The first archive whose file name contains every fragment, ignoring case.
///
/// Matched by fragment rather than by exact name because a Nexus download
/// carries a mod id, a file id and a timestamp that all change when the file is
/// downloaded again: an exact name would send this back to silently skipping
/// the first time somebody re-fetched a fixture.
pub fn find_archive(env_var: &str, subdir: &str, fragments: &[&str]) -> Option<PathBuf> {
    for dir in fixture_dirs(env_var, subdir) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".zip") {
                continue;
            }
            if fragments
                .iter()
                .all(|f| name.contains(&f.to_ascii_lowercase()))
            {
                return Some(path);
            }
        }
    }
    None
}

/// What to print when a fixture is missing, so a skip is diagnosable.
///
/// A test that skips silently is indistinguishable from one that passed, which
/// is exactly how the acceptance test went unnoticed for as long as it did.
pub fn describe_missing(what: &str, env_var: &str, subdir: &str) -> String {
    let looked: Vec<String> = fixture_dirs(env_var, subdir)
        .iter()
        .map(|d| tidy(d))
        .collect();
    format!(
        "SKIP: no {what} archive found. Looked in: {}. Set {env_var} to point somewhere else.",
        looked.join(", ")
    )
}

/// A path with its `..` segments folded away, for a readable message.
fn tidy(path: &Path) -> String {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            other => parts.push(other.as_os_str().to_os_string()),
        }
    }
    PathBuf::from_iter(parts).display().to_string()
}
