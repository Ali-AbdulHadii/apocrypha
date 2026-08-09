//! Deciding whether a downloaded file has been superseded.
//!
//! This is deliberately pure: it takes a mod's file listing and the file id we
//! actually installed, and returns a verdict. Nothing here talks to the network,
//! so every rule below is testable against a fixture rather than against Nexus
//! being in a particular mood.
//!
//! # Why the update chain rather than "the newest file"
//!
//! Picking the newest upload looks simpler and is wrong often enough to matter.
//! A mod page carries main files, optional add-ons, patches for other mods,
//! and old versions kept for compatibility, and the newest upload is frequently
//! an optional extra that has nothing to do with the file someone installed.
//! Offering it as "an update" would replace a working install with a different
//! mod.
//!
//! Nexus publishes `file_updates`, an explicit list of `old_file_id ->
//! new_file_id` edges the author creates when uploading a replacement. That is
//! the author saying "this supersedes that", which is exactly the question, so
//! following the chain is preferred and the timestamp heuristic is only a
//! fallback for mods whose authors never linked their uploads.

use crate::api::{ModFile, ModFiles};

/// What a file listing says about the file we have.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    /// Nothing newer. The installed file is still current.
    UpToDate,
    /// A newer file exists, and this is it.
    Available(Box<ModFile>),
    /// The installed file is not in the listing at all, so nothing can be
    /// compared. Usually means the author archived or deleted it.
    ///
    /// Reported rather than silently treated as "up to date", because a file
    /// that vanished from its mod page is something the user may want to know
    /// about — it is the case the vault exists for.
    Unknown,
}

/// A chain longer than this is a cycle or an author's mistake, not a history.
const MAX_CHAIN: usize = 64;

/// Categories that can legitimately replace an installed file.
///
/// `OLD_VERSION` and `ARCHIVED` are excluded because offering them is a
/// downgrade. `OPTIONAL` and `MISCELLANEOUS` are excluded from the *fallback*
/// because they are usually separate add-ons rather than replacements — but the
/// update chain overrides this, since an explicit link from the author outranks
/// a guess about the category.
fn is_replacement_category(f: &ModFile) -> bool {
    matches!(
        f.category_name.as_deref(),
        Some("MAIN") | Some("UPDATE") | Some("main") | Some("update")
    )
}

fn find(files: &[ModFile], id: u64) -> Option<&ModFile> {
    files.iter().find(|f| f.file_id == id)
}

/// Walk `old -> new` edges from `start` and return the last id reached.
///
/// Stops on a repeat so a cycle cannot hang the check, and stops at `MAX_CHAIN`
/// so a pathological listing cannot either.
fn follow_chain(files: &ModFiles, start: u64) -> u64 {
    let mut current = start;
    let mut seen = vec![start];

    for _ in 0..MAX_CHAIN {
        let next = files
            .file_updates
            .iter()
            .find(|u| u.old_file_id == current)
            .map(|u| u.new_file_id);

        match next {
            Some(n) if !seen.contains(&n) => {
                current = n;
                seen.push(n);
            }
            _ => break,
        }
    }

    current
}

/// Decide whether `current_file_id` has been superseded.
pub fn pick_update(files: &ModFiles, current_file_id: u64) -> UpdateStatus {
    let installed = find(&files.files, current_file_id);

    // The author's own "this replaces that" links come first.
    let chain_end = follow_chain(files, current_file_id);
    if chain_end != current_file_id {
        if let Some(f) = find(&files.files, chain_end) {
            return UpdateStatus::Available(Box::new(f.clone()));
        }
        // The chain names a file that is not in the listing. That is a broken
        // mod page rather than an update we can act on, so fall through to the
        // timestamp rule instead of offering a file we cannot download.
    }

    let Some(installed) = installed else {
        return UpdateStatus::Unknown;
    };

    // Fallback: the newest main-category upload, but only if it is genuinely
    // newer than what we have. Equal timestamps are not an update.
    let newest = files
        .files
        .iter()
        .filter(|f| is_replacement_category(f))
        .filter(|f| f.uploaded_timestamp > installed.uploaded_timestamp)
        .max_by_key(|f| f.uploaded_timestamp);

    match newest {
        Some(f) => UpdateStatus::Available(Box::new(f.clone())),
        None => UpdateStatus::UpToDate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::FileUpdate;

    fn file(id: u64, ts: i64, category: &str) -> ModFile {
        ModFile {
            file_id: id,
            name: format!("File {id}"),
            file_name: format!("file-{id}.zip"),
            version: Some(format!("1.{id}")),
            category_name: Some(category.to_string()),
            uploaded_timestamp: ts,
            size: 100,
        }
    }

    fn listing(files: Vec<ModFile>, updates: Vec<(u64, u64)>) -> ModFiles {
        ModFiles {
            files,
            file_updates: updates
                .into_iter()
                .map(|(old_file_id, new_file_id)| FileUpdate {
                    old_file_id,
                    new_file_id,
                    uploaded_timestamp: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn the_only_file_is_up_to_date() {
        let l = listing(vec![file(1, 100, "MAIN")], vec![]);
        assert_eq!(pick_update(&l, 1), UpdateStatus::UpToDate);
    }

    #[test]
    fn an_authors_replacement_link_is_followed() {
        let l = listing(
            vec![file(1, 100, "MAIN"), file(2, 200, "MAIN")],
            vec![(1, 2)],
        );
        match pick_update(&l, 1) {
            UpdateStatus::Available(f) => assert_eq!(f.file_id, 2),
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn the_chain_is_followed_to_its_end_not_one_step() {
        // Someone who skipped two releases should be offered the newest, not
        // the one immediately after theirs.
        let l = listing(
            vec![
                file(1, 100, "MAIN"),
                file(2, 200, "MAIN"),
                file(3, 300, "MAIN"),
            ],
            vec![(1, 2), (2, 3)],
        );
        match pick_update(&l, 1) {
            UpdateStatus::Available(f) => assert_eq!(f.file_id, 3),
            other => panic!("expected file 3, got {other:?}"),
        }
    }

    #[test]
    fn a_cycle_in_the_chain_terminates() {
        // Nothing stops an author creating a loop. It must not hang the check.
        let l = listing(
            vec![file(1, 100, "MAIN"), file(2, 200, "MAIN")],
            vec![(1, 2), (2, 1)],
        );
        // Whatever it settles on, it must return rather than spin.
        let _ = pick_update(&l, 1);
    }

    #[test]
    fn a_newer_optional_file_is_not_an_update() {
        // The single most damaging false positive: an optional add-on uploaded
        // after the main file, offered as a replacement for it.
        let l = listing(vec![file(1, 100, "MAIN"), file(2, 999, "OPTIONAL")], vec![]);
        assert_eq!(pick_update(&l, 1), UpdateStatus::UpToDate);
    }

    #[test]
    fn an_older_main_file_is_not_an_update() {
        let l = listing(vec![file(1, 500, "MAIN"), file(2, 100, "MAIN")], vec![]);
        assert_eq!(pick_update(&l, 1), UpdateStatus::UpToDate);
    }

    #[test]
    fn an_equal_timestamp_is_not_an_update() {
        let l = listing(vec![file(1, 100, "MAIN"), file(2, 100, "MAIN")], vec![]);
        assert_eq!(pick_update(&l, 1), UpdateStatus::UpToDate);
    }

    #[test]
    fn a_newer_main_file_is_an_update_without_a_chain() {
        // Plenty of authors never link their uploads, so the fallback has to
        // work on its own.
        let l = listing(vec![file(1, 100, "MAIN"), file(2, 200, "MAIN")], vec![]);
        match pick_update(&l, 1) {
            UpdateStatus::Available(f) => assert_eq!(f.file_id, 2),
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn an_optional_file_can_still_be_updated_through_the_chain() {
        // The category rule is a fallback heuristic; an explicit link from the
        // author outranks it.
        let l = listing(
            vec![file(1, 100, "OPTIONAL"), file(2, 200, "OPTIONAL")],
            vec![(1, 2)],
        );
        match pick_update(&l, 1) {
            UpdateStatus::Available(f) => assert_eq!(f.file_id, 2),
            other => panic!("expected an update, got {other:?}"),
        }
    }

    #[test]
    fn a_file_missing_from_the_listing_is_unknown_not_up_to_date() {
        let l = listing(vec![file(2, 200, "MAIN")], vec![]);
        assert_eq!(pick_update(&l, 1), UpdateStatus::Unknown);
    }

    #[test]
    fn a_chain_pointing_at_a_missing_file_falls_back_rather_than_offering_it() {
        // The listing says 1 was replaced by 9, but 9 is not there to download.
        // Offering it would produce a download that cannot start.
        let l = listing(
            vec![file(1, 100, "MAIN"), file(2, 200, "MAIN")],
            vec![(1, 9)],
        );
        match pick_update(&l, 1) {
            UpdateStatus::Available(f) => assert_eq!(f.file_id, 2, "fell back to the newest main"),
            other => panic!("expected the fallback to apply, got {other:?}"),
        }
    }
}
