//! Checking a deployment against the game directory, and putting it back.
//!
//! The journal records what we did, not what is still true. Games patch
//! themselves, other mod tools write into the same folders, and users delete
//! files by hand. Everything here exists so the manager can tell the difference
//! between "this deployment is intact" and "we no longer know", instead of
//! trusting a journal that may be months stale.
//!
//! Verification is strictly read-only, and repair only ever re-places bytes the
//! journal already vouches for, so neither pass can invalidate a rollback.

use crate::journal::{Journal, JournalOp};
use crate::{ladder_for, place, safe_dest, vault, DeployContext};
use std::collections::HashSet;
use std::fs;

/// What the game folder says about one file we deployed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Ok,
    Missing,
    Modified,
}

/// One file whose on-disk state is not what the journal describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVerdict {
    pub game_rel_path: String,
    pub state: FileState,
    /// False when the journal predates staged-path recording, so there is
    /// nothing to re-place the file from. Also false once the staged payload
    /// itself is gone: offering a repair that cannot work is worse than saying
    /// up front that this one is beyond us.
    pub repairable: bool,
}

/// The result of one verification pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Files the journal claims to have placed. Registry overrides are not
    /// files and are not counted.
    pub checked: usize,
    /// Of those, the ones that are as we left them. A loader DLL counts here on
    /// presence alone, because the journal records no hash for it: see
    /// [`verify`].
    pub ok: usize,
    pub problems: Vec<FileVerdict>,
}

impl VerifyReport {
    pub fn is_intact(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Outcome of a repair pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepairReport {
    /// Destination paths put back from staging.
    pub repaired: Vec<String>,
    /// Files deliberately not touched, each with the reason, because a skip the
    /// user cannot explain reads as a bug.
    pub skipped: Vec<String>,
    /// Files a repair was attempted on and failed, each with the reason.
    pub errors: Vec<String>,
}

/// The destination a file op acts on. `None` for ops that are not files.
fn file_path_of(op: &JournalOp) -> Option<&str> {
    match op {
        JournalOp::Created { game_rel_path, .. }
        | JournalOp::Replaced { game_rel_path, .. }
        | JournalOp::LoaderInstalled { game_rel_path, .. } => Some(game_rel_path),
        // A registry override is a value in the Proton prefix, not a file. There
        // is nothing on disk here to compare, so it is skipped outright rather
        // than counted as a file we checked.
        JournalOp::RegistryOverride { .. } => None,
        // A plugin list is a file, but not one of these. Every path here is
        // game-relative and every repair re-places from staging; the list is
        // outside the game directory and was composed rather than staged, so
        // there is no source to put back. It is also the file most likely to
        // have been legitimately edited since, which repair must not undo.
        JournalOp::PluginListWritten { .. } => None,
    }
}

/// The staged source and recorded hash of an op we placed ourselves.
///
/// `None` for [`JournalOp::LoaderInstalled`], which is copied from outside the
/// staging tree and journals no hash: presence is all we can ever know about it.
fn placed_source(op: &JournalOp) -> Option<(&str, &str)> {
    match op {
        JournalOp::Created {
            staged_rel_path,
            sha256,
            ..
        }
        | JournalOp::Replaced {
            staged_rel_path,
            sha256,
            ..
        } => Some((staged_rel_path, sha256)),
        _ => None,
    }
}

/// The ops that describe what should be on disk *now*, in journal order.
///
/// A later op at the same destination supersedes an earlier one (a loader
/// install over a path the plan had already placed), so only the last op for a
/// given file says what its bytes are supposed to be. Checking both would report
/// a phantom problem against the superseded hash.
fn effective_file_ops(journal: &Journal) -> Vec<&JournalOp> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out: Vec<&JournalOp> = Vec::new();
    for op in journal.ops().iter().rev() {
        let Some(path) = file_path_of(op) else {
            continue;
        };
        if seen.insert(path) {
            out.push(op);
        }
    }
    out.reverse();
    out
}

/// Whether [`repair`] has bytes to put this file back from.
fn can_repair(ctx: &DeployContext, op: &JournalOp) -> bool {
    match placed_source(op) {
        Some((staged, _)) if !staged.is_empty() => ctx.staging_dir.join(staged).is_file(),
        _ => false,
    }
}

/// The current state of one deployed file.
fn state_of(ctx: &DeployContext, op: &JournalOp) -> FileState {
    let Some(path) = file_path_of(op) else {
        return FileState::Ok;
    };
    let Ok(dest) = safe_dest(&ctx.game_dir, path) else {
        // A destination that escapes the game directory was never written by us,
        // so nothing of ours is there to find.
        return FileState::Missing;
    };
    if !dest.is_file() {
        return FileState::Missing;
    }
    let Some((_, sha256)) = placed_source(op) else {
        // Loader DLL: no hash was journaled, so presence is the whole check.
        // Claiming more than that would be a lie the user cannot audit.
        return FileState::Ok;
    };
    match vault::hash_file(&dest) {
        Ok(current) if current == sha256 => FileState::Ok,
        Ok(_) => FileState::Modified,
        // A file we cannot read is a file we cannot vouch for, and this pass
        // exists to say so.
        Err(_) => FileState::Modified,
    }
}

/// Compare what the journal says was deployed against what is on disk.
/// Makes no modifications.
///
/// Content is compared by hash wherever the journal recorded one. Loader DLLs
/// are journaled without a hash, so they are checked for presence only and are
/// reported as ok when present: the alternative, quietly implying their bytes
/// were verified, is the failure this whole module exists to prevent.
pub fn verify(ctx: &DeployContext, journal: &Journal) -> VerifyReport {
    let mut report = VerifyReport::default();
    for op in effective_file_ops(journal) {
        let Some(path) = file_path_of(op) else {
            continue;
        };
        report.checked += 1;
        let state = state_of(ctx, op);
        if state == FileState::Ok {
            report.ok += 1;
            continue;
        }
        report.problems.push(FileVerdict {
            game_rel_path: path.to_string(),
            state,
            repairable: can_repair(ctx, op),
        });
    }
    report
}

/// Re-place the given files from staging. The caller chooses which verdicts to
/// act on, because re-placing a Modified file overwrites whatever is there now
/// and that is a decision only the user should make.
///
/// Nothing is appended to the journal. Repair only ever writes bytes whose hash
/// already equals the one the journal recorded for that file, so the journal
/// stays an accurate description of the deployment and rollback's hash guard
/// keeps working. That check is also why a staged source that has since changed
/// is refused instead of placed: writing different bytes under the old hash
/// would leave rollback unable to clean up its own deployment.
///
/// The on-disk state is re-derived here rather than taken from the verdict,
/// because verify and repair are separated by however long the user spent
/// reading the report.
pub fn repair(ctx: &DeployContext, journal: &Journal, act_on: &[FileVerdict]) -> RepairReport {
    let ops = effective_file_ops(journal);
    let mut report = RepairReport::default();

    for verdict in act_on {
        let path = verdict.game_rel_path.as_str();
        let Some(op) = ops.iter().find(|op| file_path_of(op) == Some(path)) else {
            report
                .errors
                .push(format!("{path}: not part of this deployment"));
            continue;
        };
        let Some((staged_rel_path, sha256)) = placed_source(op) else {
            report
                .skipped
                .push(format!("{path}: the loader was not placed from staging"));
            continue;
        };
        if staged_rel_path.is_empty() {
            report.skipped.push(format!(
                "{path}: this journal predates staged-path recording, so there is no source to re-place it from"
            ));
            continue;
        }
        let src = ctx.staging_dir.join(staged_rel_path);
        if !src.is_file() {
            report
                .skipped
                .push(format!("{path}: staged source {staged_rel_path} is gone"));
            continue;
        }
        match vault::hash_file(&src) {
            Ok(current) if current == sha256 => {}
            Ok(_) => {
                report.skipped.push(format!(
                    "{path}: staged source {staged_rel_path} no longer matches the bytes this deployment recorded"
                ));
                continue;
            }
            Err(e) => {
                report.errors.push(format!("{path}: {e}"));
                continue;
            }
        }
        if state_of(ctx, op) == FileState::Ok {
            report
                .skipped
                .push(format!("{path}: already matches the journal"));
            continue;
        }

        let Ok(dest) = safe_dest(&ctx.game_dir, path) else {
            report.errors.push(format!("{path}: unsafe destination"));
            continue;
        };
        // `place` requires a free destination so a failed rung can be retried
        // without ambiguity, so whatever is there now goes first.
        if dest.exists() {
            if let Err(e) = fs::remove_file(&dest) {
                report.errors.push(format!("{path}: {e}"));
                continue;
            }
        }
        let ladder = ladder_for(ctx, path);
        match place::place(&ladder, &src, &dest) {
            Ok(_) => report.repaired.push(path.to_string()),
            Err(e) => report.errors.push(format!("{path}: {e}")),
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{new_deployment_id, now_secs, DeploymentHeader};
    use crate::place::Ladder;
    use apoc_domain::{DeployMethod, DeploymentPlan, PlannedFile};
    use std::path::Path;
    use tempfile::tempdir;

    struct Fixture {
        _dir: tempfile::TempDir,
        ctx: DeployContext,
    }

    fn fixture() -> Fixture {
        let dir = tempdir().unwrap();
        let ctx = DeployContext {
            game_id: "monster-hunter-wilds".into(),
            game_dir: dir.path().join("game"),
            staging_dir: dir.path().join("staging"),
            vault_dir: dir.path().join("vault"),
            journal_dir: dir.path().join("journal"),
            ladder: Ladder::default(),
            pak_chain: None,
            copy_only_paths: Vec::new(),
        };
        fs::create_dir_all(&ctx.game_dir).unwrap();
        fs::create_dir_all(&ctx.staging_dir).unwrap();
        Fixture { _dir: dir, ctx }
    }

    fn stage(ctx: &DeployContext, rel: &str, bytes: &[u8]) {
        let p = ctx.staging_dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn plan_of(files: &[(&str, &str, u64)]) -> DeploymentPlan {
        DeploymentPlan {
            bundle_name: "Test Bundle".into(),
            files: files
                .iter()
                .map(|(staged, game, size)| PlannedFile {
                    option_id: "opt".into(),
                    staged_rel_path: staged.to_string(),
                    game_rel_path: game.to_string(),
                    size: *size,
                })
                .collect(),
            conflicts: vec![],
            issues: vec![],
        }
    }

    fn header() -> DeploymentHeader {
        DeploymentHeader {
            id: new_deployment_id(),
            game_id: "monster-hunter-wilds".into(),
            bundle_name: "Hand-written".into(),
            created_at: now_secs(),
            game_dir: "/games/mhw".into(),
        }
    }

    /// Rewrite a deployed file the way something outside Apocrypha would: the
    /// file is removed first so a hardlinked deployment does not also change the
    /// staged source.
    fn external_edit(path: &Path, bytes: &[u8]) {
        fs::remove_file(path).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn an_intact_deployment_verifies_clean() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"AAAA");
        stage(&f.ctx, "opt/natives/b.pak", b"BBBB");
        let plan = plan_of(&[
            ("opt/natives/a.pak", "natives/a.pak", 4),
            ("opt/natives/b.pak", "natives/b.pak", 4),
        ]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        let report = verify(&f.ctx, &journal);
        assert!(report.is_intact(), "{report:?}");
        assert_eq!(report.checked, 2);
        assert_eq!(report.ok, 2);
    }

    #[test]
    fn a_deleted_file_reads_as_missing_and_is_repaired_back_to_the_right_bytes() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"MODDED BYTES");
        let plan = plan_of(&[("opt/natives/a.pak", "natives/a.pak", 12)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        let deployed = f.ctx.game_dir.join("natives/a.pak");
        fs::remove_file(&deployed).unwrap();

        let report = verify(&f.ctx, &journal);
        assert!(!report.is_intact());
        assert_eq!(
            report.problems,
            vec![FileVerdict {
                game_rel_path: "natives/a.pak".into(),
                state: FileState::Missing,
                repairable: true,
            }]
        );

        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert_eq!(repaired.repaired, vec!["natives/a.pak".to_string()]);
        assert!(repaired.errors.is_empty(), "{repaired:?}");
        assert!(repaired.skipped.is_empty(), "{repaired:?}");
        assert_eq!(fs::read(&deployed).unwrap(), b"MODDED BYTES");
        assert!(
            verify(&f.ctx, &journal).is_intact(),
            "repair restores trust"
        );
    }

    #[test]
    fn an_externally_edited_file_reads_as_modified_and_is_left_alone_until_it_is_passed_to_repair()
    {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"OURS");
        stage(&f.ctx, "opt/natives/b.pak", b"BBBB");
        let plan = plan_of(&[
            ("opt/natives/a.pak", "natives/a.pak", 4),
            ("opt/natives/b.pak", "natives/b.pak", 4),
        ]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        let deployed = f.ctx.game_dir.join("natives/a.pak");
        external_edit(&deployed, b"EDITED BY SOMEONE ELSE");

        let report = verify(&f.ctx, &journal);
        assert_eq!(report.checked, 2);
        assert_eq!(report.ok, 1, "the untouched file still verifies");
        assert_eq!(
            report.problems,
            vec![FileVerdict {
                game_rel_path: "natives/a.pak".into(),
                state: FileState::Modified,
                repairable: true,
            }]
        );
        assert_eq!(
            fs::read(&deployed).unwrap(),
            b"EDITED BY SOMEONE ELSE",
            "verify never writes"
        );

        // Not acting on the verdict leaves the user's bytes exactly where they are.
        let untouched = repair(&f.ctx, &journal, &[]);
        assert_eq!(untouched, RepairReport::default());
        assert_eq!(fs::read(&deployed).unwrap(), b"EDITED BY SOMEONE ELSE");

        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert_eq!(repaired.repaired, vec!["natives/a.pak".to_string()]);
        assert_eq!(fs::read(&deployed).unwrap(), b"OURS");
    }

    #[test]
    fn a_journal_without_a_staged_path_reports_the_file_as_unrepairable() {
        let f = fixture();
        let deployed = f.ctx.game_dir.join("natives/old.pak");
        fs::create_dir_all(deployed.parent().unwrap()).unwrap();
        fs::write(&deployed, b"DEPLOYED LONG AGO").unwrap();
        let sha256 = vault::hash_file(&deployed).unwrap();

        let mut journal = Journal::create(&f.ctx.journal_dir, header()).unwrap();
        journal
            .append(JournalOp::Created {
                game_rel_path: "natives/old.pak".into(),
                staged_rel_path: String::new(),
                sha256,
                method: DeployMethod::Hardlink,
            })
            .unwrap();
        fs::remove_file(&deployed).unwrap();

        let report = verify(&f.ctx, &journal);
        assert_eq!(
            report.problems,
            vec![FileVerdict {
                game_rel_path: "natives/old.pak".into(),
                state: FileState::Missing,
                repairable: false,
            }],
            "an unrepairable file is still reported, just not offered a fix"
        );

        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert!(repaired.repaired.is_empty());
        assert!(repaired.errors.is_empty(), "{repaired:?}");
        assert_eq!(
            repaired.skipped.len(),
            1,
            "the skip is surfaced, not dropped"
        );
        assert!(repaired.skipped[0].starts_with("natives/old.pak:"));
    }

    #[test]
    fn a_staged_source_that_is_gone_makes_the_file_unrepairable_rather_than_failing_later() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"AAAA");
        let plan = plan_of(&[("opt/natives/a.pak", "natives/a.pak", 4)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        fs::remove_file(f.ctx.game_dir.join("natives/a.pak")).unwrap();
        fs::remove_file(f.ctx.staging_dir.join("opt/natives/a.pak")).unwrap();

        let report = verify(&f.ctx, &journal);
        assert_eq!(report.problems[0].state, FileState::Missing);
        assert!(!report.problems[0].repairable);

        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert!(repaired.repaired.is_empty());
        assert_eq!(repaired.skipped.len(), 1);
    }

    #[test]
    fn a_staged_source_whose_bytes_changed_is_refused_so_rollback_keeps_working() {
        let f = fixture();
        stage(&f.ctx, "opt/dinput8.dll", b"LOADER V1");
        // A copy-only ladder file, so rewriting the staged source cannot reach
        // the deployed file through a shared inode.
        let plan = plan_of(&[("opt/dinput8.dll", "dinput8.dll", 9)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        fs::remove_file(f.ctx.game_dir.join("dinput8.dll")).unwrap();
        fs::write(f.ctx.staging_dir.join("opt/dinput8.dll"), b"LOADER V2").unwrap();

        let report = verify(&f.ctx, &journal);
        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert!(
            repaired.repaired.is_empty(),
            "bytes the journal does not vouch for are never placed"
        );
        assert_eq!(repaired.skipped.len(), 1);
        assert!(
            !f.ctx.game_dir.join("dinput8.dll").exists(),
            "the refusal leaves the game directory alone"
        );
    }

    #[test]
    fn a_loader_op_is_checked_for_presence_only_because_no_hash_was_recorded() {
        let f = fixture();
        let dll = f.ctx.game_dir.join("dinput8.dll");
        fs::write(&dll, b"LOADER").unwrap();

        let mut journal = Journal::create(&f.ctx.journal_dir, header()).unwrap();
        journal
            .append(JournalOp::LoaderInstalled {
                game_rel_path: "dinput8.dll".into(),
                original_vault_key: None,
            })
            .unwrap();

        // A different loader build is not something we can detect, and verify
        // does not pretend otherwise.
        fs::write(&dll, b"A DIFFERENT LOADER BUILD").unwrap();
        let report = verify(&f.ctx, &journal);
        assert_eq!((report.checked, report.ok), (1, 1));
        assert!(report.is_intact());

        fs::remove_file(&dll).unwrap();
        let report = verify(&f.ctx, &journal);
        assert_eq!(
            report.problems,
            vec![FileVerdict {
                game_rel_path: "dinput8.dll".into(),
                state: FileState::Missing,
                repairable: false,
            }],
            "absence is the one thing presence-checking can prove"
        );
    }

    #[test]
    fn registry_overrides_are_not_counted_as_files() {
        let f = fixture();
        let mut journal = Journal::create(&f.ctx.journal_dir, header()).unwrap();
        journal
            .append(JournalOp::RegistryOverride {
                name: "dinput8".into(),
                value: "native,builtin".into(),
                previous: None,
            })
            .unwrap();

        let report = verify(&f.ctx, &journal);
        assert_eq!(report.checked, 0);
        assert!(report.is_intact());
    }

    #[test]
    fn only_the_last_op_for_a_destination_decides_what_should_be_there() {
        let f = fixture();
        let dll = f.ctx.game_dir.join("dinput8.dll");
        fs::write(&dll, b"THE LOADER THAT IS THERE NOW").unwrap();

        let mut journal = Journal::create(&f.ctx.journal_dir, header()).unwrap();
        journal
            .append(JournalOp::Created {
                game_rel_path: "dinput8.dll".into(),
                staged_rel_path: "opt/dinput8.dll".into(),
                sha256: "a-hash-of-something-else".into(),
                method: DeployMethod::Copy,
            })
            .unwrap();
        journal
            .append(JournalOp::LoaderInstalled {
                game_rel_path: "dinput8.dll".into(),
                original_vault_key: None,
            })
            .unwrap();

        let report = verify(&f.ctx, &journal);
        assert_eq!(report.checked, 1, "one destination is one check");
        assert!(
            report.is_intact(),
            "the superseded hash must not raise a phantom problem: {report:?}"
        );
    }

    #[test]
    fn repair_does_not_append_to_the_journal_so_rollback_still_works() {
        let f = fixture();
        let vanilla = f.ctx.game_dir.join("natives/shared.pak");
        fs::create_dir_all(vanilla.parent().unwrap()).unwrap();
        fs::write(&vanilla, b"VANILLA").unwrap();
        stage(&f.ctx, "opt/natives/shared.pak", b"MODDED");
        let plan = plan_of(&[("opt/natives/shared.pak", "natives/shared.pak", 6)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();
        let lines_before = fs::read_to_string(journal.path()).unwrap().lines().count();

        fs::remove_file(&vanilla).unwrap();
        let report = verify(&f.ctx, &journal);
        let repaired = repair(&f.ctx, &journal, &report.problems);
        assert_eq!(repaired.repaired, vec!["natives/shared.pak".to_string()]);

        assert_eq!(
            fs::read_to_string(journal.path()).unwrap().lines().count(),
            lines_before,
            "a repair records nothing: it only restores what the journal already describes"
        );

        let rollback = crate::rollback(&f.ctx, &journal, None);
        assert!(rollback.is_clean(), "{rollback:?}");
        assert_eq!(
            fs::read(&vanilla).unwrap(),
            b"VANILLA",
            "the original is still restorable after a repair"
        );
    }

    #[test]
    fn a_file_from_another_deployment_is_reported_as_an_error_not_repaired() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"AAAA");
        let plan = plan_of(&[("opt/natives/a.pak", "natives/a.pak", 4)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        let repaired = repair(
            &f.ctx,
            &journal,
            &[FileVerdict {
                game_rel_path: "natives/not-ours.pak".into(),
                state: FileState::Missing,
                repairable: true,
            }],
        );
        assert!(repaired.repaired.is_empty());
        assert_eq!(repaired.errors.len(), 1);
        assert!(repaired.errors[0].contains("not part of this deployment"));
    }

    #[test]
    fn repairing_a_file_that_is_already_correct_changes_nothing() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/a.pak", b"AAAA");
        let plan = plan_of(&[("opt/natives/a.pak", "natives/a.pak", 4)]);
        let journal = crate::apply(&f.ctx, &plan).unwrap();

        // A stale verdict: the user fixed the file themselves while the report
        // sat on screen.
        let repaired = repair(
            &f.ctx,
            &journal,
            &[FileVerdict {
                game_rel_path: "natives/a.pak".into(),
                state: FileState::Missing,
                repairable: true,
            }],
        );
        assert!(repaired.repaired.is_empty());
        assert_eq!(repaired.skipped.len(), 1);
        assert!(repaired.skipped[0].contains("already matches"));
    }
}
