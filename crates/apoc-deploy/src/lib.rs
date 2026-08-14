//! The deployment engine.
//!
//! Applies a [`DeploymentPlan`] to a game directory with three safety invariants:
//!
//! 1. **Nothing is destroyed silently.** A pre-existing file is copied into the
//!    content-addressed [`vault`] before being replaced.
//! 2. **Everything is journaled before it counts.** Each op is flushed to the
//!    [`journal`] as it happens, so an interrupted deploy is still reversible.
//! 3. **Deletion is hash-guarded.** Rollback refuses to remove a file whose bytes
//!    no longer match what we deployed (e.g. a Steam update replaced it).

pub mod journal;
pub mod loader;
pub mod place;
pub mod vault;
pub mod verify;

use apoc_domain::{DeployMethod, DeploymentPlan, PakChainSpec, PlannedFile};
use journal::{DeploymentHeader, Journal, JournalOp};
use place::Ladder;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use vault::Vault;

#[derive(Debug, Error)]
pub enum DeployError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("plan is not installable: {0}")]
    InvalidPlan(String),
    #[error("staged file missing: {0}")]
    MissingStagedFile(PathBuf),
    #[error("refusing to modify the game while Steam is running")]
    SteamRunning,
    #[error("destination escapes the game directory: {0}")]
    UnsafeDestination(String),
}

pub type Result<T> = std::result::Result<T, DeployError>;

/// Everything the engine needs to act on one game.
#[derive(Debug, Clone)]
pub struct DeployContext {
    pub game_id: String,
    /// The game install directory (deployment target).
    pub game_dir: PathBuf,
    /// Root of this mod's staged payload.
    pub staging_dir: PathBuf,
    /// Where original files are vaulted.
    pub vault_dir: PathBuf,
    /// Where deployment journals are written.
    pub journal_dir: PathBuf,
    pub ladder: Ladder,
    /// How standalone `.pak` mods are named into the engine's patch chain.
    /// `None` for games that do not support PAK mods.
    pub pak_chain: Option<PakChainSpec>,
    /// Game-relative destinations that must be real copies, never links.
    ///
    /// Populate from the active profile's proxy DLLs. This exists because the
    /// heuristic below cannot express Cyberpunk: it asks whether a path has no
    /// `/` in it, which is true of REFramework's root-level `dinput8.dll` and
    /// false of both of Cyberpunk's, which live at `bin/x64/`. So the proxies
    /// were being hardlinked while `place.rs` said they were "always a real
    /// copy" and `apoc game cyberpunk-2077` printed the same promise.
    ///
    /// Keeping the game-specific answer in data rather than in a branch here is
    /// also what stops this crate learning the names of games.
    pub copy_only_paths: Vec<String>,
}

impl DeployContext {
    /// Game-relative destinations that must be real copies, taken from a
    /// profile's loader. Use it to fill [`Self::copy_only_paths`].
    pub fn copy_only_from(profile: &apoc_domain::GameProfile) -> Vec<String> {
        profile
            .loader
            .as_ref()
            .map(|l| l.proxy_dlls().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

/// What a plan *would* do, without touching anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRun {
    pub method: DeployMethod,
    pub creates: Vec<String>,
    /// Existing game files that would be replaced (and vaulted first).
    pub replaces: Vec<String>,
    pub total_bytes: u64,
    /// Staged files the plan references but that are not on disk.
    pub missing: Vec<String>,
}

impl DryRun {
    pub fn file_count(&self) -> usize {
        self.creates.len() + self.replaces.len()
    }
}

/// Loader proxy DLLs are always placed as **real copies**, never links. Wine
/// resolves the proxy DLL before the game starts and link indirection there is a
/// known source of loaders silently not loading, so the small disk cost buys
/// reliability at the exact point Linux modding fails.
///
/// A root-level `.dll` is the fallback answer, kept because it is what the
/// profile-free [`GameRules::default`](apoc_modengine) path relies on and it is
/// correct for REFramework. It is only a heuristic: it cannot see a proxy in a
/// subdirectory, which is where both of Cyberpunk's live. When the profile says
/// which paths are proxies, [`DeployContext::copy_only_paths`] answers instead.
fn is_loader_file(ctx: &DeployContext, game_rel_path: &str) -> bool {
    if ctx
        .copy_only_paths
        .iter()
        .any(|p| p.eq_ignore_ascii_case(game_rel_path))
    {
        return true;
    }
    !game_rel_path.contains('/')
        && std::path::Path::new(game_rel_path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
}

/// The ladder to use for one file: copy-only for loader DLLs, else the context's.
fn ladder_for<'a>(ctx: &'a DeployContext, game_rel_path: &str) -> std::borrow::Cow<'a, Ladder> {
    if is_loader_file(ctx, game_rel_path) {
        std::borrow::Cow::Owned(Ladder::copy_only())
    } else {
        std::borrow::Cow::Borrowed(&ctx.ladder)
    }
}

/// A planned file's destination is a bare `.pak`, meaning it still needs a
/// patch-chain name. Files already named into the chain are left alone.
fn needs_pak_name(chain: &PakChainSpec, game_rel_path: &str) -> bool {
    !game_rel_path.contains('/')
        && std::path::Path::new(game_rel_path)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        && chain.index_of(game_rel_path).is_none()
}

/// Highest patch index already present in the game directory, if any.
fn highest_pak_index(game_dir: &Path, chain: &PakChainSpec) -> Option<u32> {
    fs::read_dir(game_dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| chain.index_of(n)))
        .max()
}

/// Rewrite bare `.pak` destinations into the engine's patch chain.
///
/// RE Engine only loads a mod PAK if its filename continues the chain, so the
/// index is assigned here: above every patch already on disk: rather than at
/// parse time, when neither the game directory nor sibling mods are known.
fn resolve_pak_names(ctx: &DeployContext, plan: &DeploymentPlan) -> Vec<PlannedFile> {
    let Some(chain) = &ctx.pak_chain else {
        return plan.files.clone();
    };
    let mut next = highest_pak_index(&ctx.game_dir, chain)
        .map(|m| m + 1)
        .unwrap_or(chain.start_index)
        .max(chain.start_index);

    plan.files
        .iter()
        .map(|f| {
            if !needs_pak_name(chain, &f.game_rel_path) {
                return f.clone();
            }
            let name = chain.filename(next);
            next += 1;
            PlannedFile {
                game_rel_path: name,
                ..f.clone()
            }
        })
        .collect()
}

/// Reject destinations that escape the game directory.
fn safe_dest(game_dir: &Path, rel: &str) -> Result<PathBuf> {
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(DeployError::UnsafeDestination(rel.to_string()));
    }
    let mut out = game_dir.to_path_buf();
    for comp in candidate.components() {
        match comp {
            std::path::Component::Normal(seg) => out.push(seg),
            std::path::Component::CurDir => {}
            _ => return Err(DeployError::UnsafeDestination(rel.to_string())),
        }
    }
    if !out.starts_with(game_dir) {
        return Err(DeployError::UnsafeDestination(rel.to_string()));
    }
    Ok(out)
}

/// Compute what applying `plan` would change. Makes no modifications.
pub fn dry_run(ctx: &DeployContext, plan: &DeploymentPlan) -> Result<DryRun> {
    let method =
        place::probe(&ctx.ladder, &ctx.staging_dir, &ctx.game_dir).unwrap_or(DeployMethod::Copy);

    let mut creates = Vec::new();
    let mut replaces = Vec::new();
    let mut missing = Vec::new();

    let files = resolve_pak_names(ctx, plan);
    for f in &files {
        let src = ctx.staging_dir.join(&f.staged_rel_path);
        if !src.is_file() {
            missing.push(f.staged_rel_path.clone());
            continue;
        }
        let dest = safe_dest(&ctx.game_dir, &f.game_rel_path)?;
        if dest.exists() {
            replaces.push(f.game_rel_path.clone());
        } else {
            creates.push(f.game_rel_path.clone());
        }
    }

    Ok(DryRun {
        method,
        creates,
        replaces,
        total_bytes: plan.total_size(),
        missing,
    })
}

/// How far an apply has got. Counts are over planned files, so a caller can
/// render both a file count and a byte total.
#[derive(Debug, Clone, Default)]
pub struct ApplyProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Destination path of the file just written.
    pub current: String,
}

/// What the caller wants to happen next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Cancel,
}

/// How an apply ended.
#[derive(Debug)]
pub enum Applied {
    Complete(Journal),
    /// The caller asked to stop. Everything already written has been rolled
    /// back, so the game directory is as it was before the apply started.
    Cancelled {
        journal: Journal,
        rollback: RollbackReport,
    },
}

/// Apply a plan. Returns the journal describing exactly what was done.
pub fn apply(ctx: &DeployContext, plan: &DeploymentPlan) -> Result<Journal> {
    match apply_with(ctx, plan, &mut |_| Flow::Continue)? {
        Applied::Complete(journal) => Ok(journal),
        // Unreachable with a sink that never cancels, but returning the journal
        // keeps the old signature honest instead of panicking on a refactor.
        Applied::Cancelled { journal, .. } => Ok(journal),
    }
}

/// Apply a plan, reporting progress and letting the caller stop.
///
/// `on_progress` is called once per file, after it is placed *and* journaled, so
/// anything it has been told about is already reversible. Returning
/// [`Flow::Cancel`] rolls back everything written so far: a cancelled apply
/// leaves the game directory exactly as it was found, which is what makes
/// cancelling safe to offer in the UI at all.
pub fn apply_with(
    ctx: &DeployContext,
    plan: &DeploymentPlan,
    on_progress: &mut dyn FnMut(&ApplyProgress) -> Flow,
) -> Result<Applied> {
    if !plan.is_valid() {
        let why = plan
            .issues
            .first()
            .map(|i| i.message.clone())
            .unwrap_or_else(|| "plan has no files".into());
        return Err(DeployError::InvalidPlan(why));
    }

    let vault = Vault::new(&ctx.vault_dir);
    let mut journal = Journal::create(
        &ctx.journal_dir,
        DeploymentHeader {
            id: journal::new_deployment_id(),
            game_id: ctx.game_id.clone(),
            bundle_name: plan.bundle_name.clone(),
            created_at: journal::now_secs(),
            game_dir: ctx.game_dir.display().to_string(),
        },
    )?;

    let files = resolve_pak_names(ctx, plan);
    let mut progress = ApplyProgress {
        files_total: files.len(),
        bytes_total: files.iter().map(|f| f.size).sum(),
        ..ApplyProgress::default()
    };
    let mut flow = Flow::Continue;

    for f in &files {
        // Checked before any work for this file, so a cancel costs at most the
        // rollback of what the caller has already been shown.
        if flow == Flow::Cancel {
            break;
        }

        let src = ctx.staging_dir.join(&f.staged_rel_path);
        if !src.is_file() {
            return Err(DeployError::MissingStagedFile(src));
        }
        let dest = safe_dest(&ctx.game_dir, &f.game_rel_path)?;

        // Vault any pre-existing original BEFORE removing it.
        let original_key = if dest.exists() {
            let key = vault.store(&dest)?;
            fs::remove_file(&dest)?;
            Some(key)
        } else {
            None
        };

        let ladder = ladder_for(ctx, &f.game_rel_path);
        let method = place::place(&ladder, &src, &dest)?;
        let sha256 = vault::hash_file(&dest)?;

        journal.append(match original_key {
            Some(original_vault_key) => JournalOp::Replaced {
                game_rel_path: f.game_rel_path.clone(),
                staged_rel_path: f.staged_rel_path.clone(),
                original_vault_key,
                sha256,
                method,
            },
            None => JournalOp::Created {
                game_rel_path: f.game_rel_path.clone(),
                staged_rel_path: f.staged_rel_path.clone(),
                sha256,
                method,
            },
        })?;

        progress.files_done += 1;
        progress.bytes_done += f.size;
        progress.current = f.game_rel_path.clone();
        flow = on_progress(&progress);
    }

    // A cancel on the last file is honoured too: whether the click landed before
    // or after the final write is invisible to the user, so "cancel" must not
    // sometimes mean "deployed".
    if flow == Flow::Cancel {
        let report = rollback(ctx, &journal, None);
        return Ok(Applied::Cancelled {
            journal,
            rollback: report,
        });
    }

    Ok(Applied::Complete(journal))
}

/// Install the loader proxy DLL (always a real copy) and register the Wine DLL
/// override in the Proton prefix. Appends to an existing deployment journal.
pub fn provision_loader(
    ctx: &DeployContext,
    journal: &mut Journal,
    loader_dll_src: &Path,
    proxy_dll_name: &str,
    user_reg: Option<&Path>,
    override_value: &str,
) -> Result<()> {
    if loader::steam_is_running() {
        return Err(DeployError::SteamRunning);
    }

    let vault = Vault::new(&ctx.vault_dir);
    let dest = safe_dest(&ctx.game_dir, proxy_dll_name)?;

    let original_vault_key = if dest.exists() {
        let key = vault.store(&dest)?;
        fs::remove_file(&dest)?;
        Some(key)
    } else {
        None
    };
    // The loader is never linked: a real copy, per the deployment design.
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(loader_dll_src, &dest)?;
    journal.append(JournalOp::LoaderInstalled {
        game_rel_path: proxy_dll_name.to_string(),
        original_vault_key,
    })?;

    if let Some(reg) = user_reg {
        // Wine keys DllOverrides by module name. A proxy that lives in a
        // subdirectory (`bin/x64/winmm.dll`) still registers as `winmm`.
        let file_name = proxy_dll_name.rsplit('/').next().unwrap_or(proxy_dll_name);
        let dll_stem = file_name.trim_end_matches(".dll");
        let previous = loader::write_override(reg, dll_stem, override_value)?;
        journal.append(JournalOp::RegistryOverride {
            name: dll_stem.to_string(),
            value: override_value.to_string(),
            previous,
        })?;
    }

    Ok(())
}

/// Outcome of a rollback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RollbackReport {
    pub removed: Vec<String>,
    pub restored: Vec<String>,
    /// Files left in place because their bytes no longer match what we deployed.
    pub skipped_modified: Vec<String>,
    pub errors: Vec<String>,
}

impl RollbackReport {
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.skipped_modified.is_empty()
    }
}

/// Reverse a deployment: restore vaulted originals, remove files we created, and
/// undo the loader override. Ops are replayed in reverse order.
///
/// A file whose current hash differs from what we recorded writing is **left
/// alone** and reported: we never delete something the user or Steam changed.
pub fn rollback(ctx: &DeployContext, journal: &Journal, user_reg: Option<&Path>) -> RollbackReport {
    let vault = Vault::new(&ctx.vault_dir);
    let mut report = RollbackReport::default();

    for op in journal.ops().iter().rev() {
        match op {
            JournalOp::Created {
                game_rel_path,
                sha256,
                ..
            } => {
                let Ok(dest) = safe_dest(&ctx.game_dir, game_rel_path) else {
                    report.errors.push(format!("unsafe path {game_rel_path}"));
                    continue;
                };
                if !dest.exists() {
                    continue;
                }
                match vault::hash_file(&dest) {
                    Ok(current) if &current == sha256 => match fs::remove_file(&dest) {
                        Ok(()) => report.removed.push(game_rel_path.clone()),
                        Err(e) => report.errors.push(format!("{game_rel_path}: {e}")),
                    },
                    Ok(_) => report.skipped_modified.push(game_rel_path.clone()),
                    Err(e) => report.errors.push(format!("{game_rel_path}: {e}")),
                }
            }
            JournalOp::Replaced {
                game_rel_path,
                original_vault_key,
                ..
            } => {
                let Ok(dest) = safe_dest(&ctx.game_dir, game_rel_path) else {
                    report.errors.push(format!("unsafe path {game_rel_path}"));
                    continue;
                };
                match vault.restore(original_vault_key, &dest) {
                    Ok(()) => report.restored.push(game_rel_path.clone()),
                    Err(e) => report.errors.push(format!("{game_rel_path}: {e}")),
                }
            }
            JournalOp::LoaderInstalled {
                game_rel_path,
                original_vault_key,
            } => {
                let Ok(dest) = safe_dest(&ctx.game_dir, game_rel_path) else {
                    report.errors.push(format!("unsafe path {game_rel_path}"));
                    continue;
                };
                match original_vault_key {
                    Some(key) => match vault.restore(key, &dest) {
                        Ok(()) => report.restored.push(game_rel_path.clone()),
                        Err(e) => report.errors.push(format!("{game_rel_path}: {e}")),
                    },
                    None => {
                        if dest.exists() {
                            match fs::remove_file(&dest) {
                                Ok(()) => report.removed.push(game_rel_path.clone()),
                                Err(e) => report.errors.push(format!("{game_rel_path}: {e}")),
                            }
                        }
                    }
                }
            }
            JournalOp::RegistryOverride { name, previous, .. } => {
                if let Some(reg) = user_reg {
                    if let Err(e) = loader::restore_override(reg, name, previous.as_deref()) {
                        report.errors.push(format!("registry {name}: {e}"));
                    } else {
                        report.restored.push(format!("prefix override {name}"));
                    }
                }
            }
        }
    }

    // Prune directories we emptied, without touching non-empty ones.
    prune_empty_dirs(&ctx.game_dir);
    report
}

/// Remove now-empty directories under `root` (bottom-up), never `root` itself.
fn prune_empty_dirs(root: &Path) {
    fn walk(dir: &Path, root: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root);
            }
        }
        if dir != root {
            if let Ok(mut it) = fs::read_dir(dir) {
                if it.next().is_none() {
                    let _ = fs::remove_dir(dir);
                }
            }
        }
    }
    walk(root, root);
}

/// List deployment journals in a journal directory, newest first.
pub fn list_journals(journal_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(journal_dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    out.sort();
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{DeploymentPlan, PlannedFile};
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
            // Empty, so these tests keep exercising the root-DLL fallback.
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

    #[test]
    fn deploys_then_rolls_back_to_pristine() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/stm/a.mesh.241111606", b"modded");
        let plan = plan_of(&[(
            "opt/natives/stm/a.mesh.241111606",
            "natives/stm/a.mesh.241111606",
            6,
        )]);

        let journal = apply(&f.ctx, &plan).unwrap();
        let deployed = f.ctx.game_dir.join("natives/stm/a.mesh.241111606");
        assert!(deployed.exists());
        assert_eq!(fs::read(&deployed).unwrap(), b"modded");

        let report = rollback(&f.ctx, &journal, None);
        assert!(report.is_clean(), "{report:?}");
        assert!(!deployed.exists(), "created file removed");
        assert!(
            !f.ctx.game_dir.join("natives").exists(),
            "emptied directories pruned"
        );
    }

    #[test]
    fn vaults_and_restores_a_replaced_original() {
        let f = fixture();
        let target = f.ctx.game_dir.join("natives/stm/shared.mdf2.45");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"VANILLA").unwrap();

        stage(&f.ctx, "opt/natives/stm/shared.mdf2.45", b"MODDED");
        let plan = plan_of(&[(
            "opt/natives/stm/shared.mdf2.45",
            "natives/stm/shared.mdf2.45",
            6,
        )]);

        let journal = apply(&f.ctx, &plan).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"MODDED");

        let report = rollback(&f.ctx, &journal, None);
        assert!(report.is_clean(), "{report:?}");
        assert_eq!(
            fs::read(&target).unwrap(),
            b"VANILLA",
            "original game file restored byte-exact"
        );
    }

    #[test]
    fn rollback_refuses_to_delete_a_file_changed_after_deploy() {
        let f = fixture();
        stage(&f.ctx, "opt/natives/x.pak", b"ours");
        let plan = plan_of(&[("opt/natives/x.pak", "natives/x.pak", 4)]);
        let journal = apply(&f.ctx, &plan).unwrap();

        // Something else (a game update, the user) rewrites the file.
        let deployed = f.ctx.game_dir.join("natives/x.pak");
        fs::remove_file(&deployed).unwrap(); // break the hardlink
        fs::write(&deployed, b"CHANGED BY SOMEONE ELSE").unwrap();

        let report = rollback(&f.ctx, &journal, None);
        assert_eq!(report.skipped_modified, vec!["natives/x.pak".to_string()]);
        assert!(deployed.exists(), "modified file must be preserved");
        assert!(!report.is_clean(), "the skip is surfaced, not silent");
    }

    #[test]
    fn dry_run_distinguishes_creates_from_replaces_and_makes_no_changes() {
        let f = fixture();
        let existing = f.ctx.game_dir.join("natives/old.pak");
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, b"vanilla").unwrap();

        stage(&f.ctx, "opt/natives/old.pak", b"new");
        stage(&f.ctx, "opt/natives/fresh.pak", b"new");
        let plan = plan_of(&[
            ("opt/natives/old.pak", "natives/old.pak", 3),
            ("opt/natives/fresh.pak", "natives/fresh.pak", 3),
        ]);

        let dr = dry_run(&f.ctx, &plan).unwrap();
        assert_eq!(dr.replaces, vec!["natives/old.pak".to_string()]);
        assert_eq!(dr.creates, vec!["natives/fresh.pak".to_string()]);
        assert!(dr.missing.is_empty());
        assert_eq!(
            fs::read(&existing).unwrap(),
            b"vanilla",
            "dry run must not modify anything"
        );
        assert!(!f.ctx.game_dir.join("natives/fresh.pak").exists());
    }

    #[test]
    fn rejects_paths_escaping_the_game_directory() {
        let f = fixture();
        stage(&f.ctx, "opt/evil", b"x");
        let plan = plan_of(&[("opt/evil", "../../etc/passwd", 1)]);
        assert!(matches!(
            apply(&f.ctx, &plan),
            Err(DeployError::UnsafeDestination(_))
        ));
    }

    #[test]
    fn loader_dlls_are_always_real_copies() {
        let f = fixture();
        stage(&f.ctx, "opt/dinput8.dll", b"LOADER");
        stage(&f.ctx, "opt/natives/x.pak", b"asset");
        let plan = plan_of(&[
            ("opt/dinput8.dll", "dinput8.dll", 6),
            ("opt/natives/x.pak", "natives/x.pak", 5),
        ]);

        let journal = apply(&f.ctx, &plan).unwrap();
        let method_for = |p: &str| {
            journal.ops().iter().find_map(|op| match op {
                JournalOp::Created {
                    game_rel_path,
                    method,
                    ..
                } if game_rel_path == p => Some(*method),
                _ => None,
            })
        };

        assert_eq!(
            method_for("dinput8.dll"),
            Some(DeployMethod::Copy),
            "the loader must never be linked"
        );

        // The copy is independent: changing the staged source leaves it intact.
        let staged = f.ctx.staging_dir.join("opt/dinput8.dll");
        fs::write(&staged, b"CHANGED").unwrap();
        assert_eq!(
            fs::read(f.ctx.game_dir.join("dinput8.dll")).unwrap(),
            b"LOADER"
        );

        // Ordinary assets still use the fast path.
        assert!(matches!(
            method_for("natives/x.pak"),
            Some(DeployMethod::Reflink) | Some(DeployMethod::Hardlink) | Some(DeployMethod::Copy)
        ));
    }

    fn wilds_chain() -> PakChainSpec {
        PakChainSpec {
            pattern: "re_chunk_000.pak.sub_000.pak.patch_{n}.pak".into(),
            digits: 3,
            start_index: 1,
        }
    }

    #[test]
    fn pak_mods_are_renamed_into_the_patch_chain() {
        let mut f = fixture();
        f.ctx.pak_chain = Some(wilds_chain());
        stage(&f.ctx, "opt/VerRBodyTextures-0-basic.pak", b"PAKDATA");
        let plan = plan_of(&[(
            "opt/VerRBodyTextures-0-basic.pak",
            "VerRBodyTextures-0-basic.pak",
            7,
        )]);

        let journal = apply(&f.ctx, &plan).unwrap();

        // The mod's own filename is never what lands in the game directory.
        assert!(!f.ctx.game_dir.join("VerRBodyTextures-0-basic.pak").exists());
        let deployed = f
            .ctx
            .game_dir
            .join("re_chunk_000.pak.sub_000.pak.patch_001.pak");
        assert!(deployed.is_file(), "pak must join the chain to be loaded");
        assert_eq!(fs::read(&deployed).unwrap(), b"PAKDATA");

        // Rollback removes the chain name it actually wrote.
        let report = rollback(&f.ctx, &journal, None);
        assert!(report.is_clean(), "{report:?}");
        assert!(!deployed.exists());
    }

    #[test]
    fn pak_indices_start_above_what_is_already_installed() {
        let mut f = fixture();
        f.ctx.pak_chain = Some(wilds_chain());
        // Vanilla/other-mod patches already present.
        for i in [1u32, 2, 5] {
            fs::write(f.ctx.game_dir.join(wilds_chain().filename(i)), b"existing").unwrap();
        }
        stage(&f.ctx, "opt/a.pak", b"A");
        stage(&f.ctx, "opt/b.pak", b"B");
        let plan = plan_of(&[("opt/a.pak", "a.pak", 1), ("opt/b.pak", "b.pak", 1)]);

        apply(&f.ctx, &plan).unwrap();

        assert!(f.ctx.game_dir.join(wilds_chain().filename(6)).is_file());
        assert!(f.ctx.game_dir.join(wilds_chain().filename(7)).is_file());
        // Existing patches are untouched.
        assert_eq!(
            fs::read(f.ctx.game_dir.join(wilds_chain().filename(5))).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn without_a_chain_spec_paks_keep_their_original_name() {
        let f = fixture(); // pak_chain: None
        stage(&f.ctx, "opt/mod.pak", b"P");
        let plan = plan_of(&[("opt/mod.pak", "mod.pak", 1)]);
        apply(&f.ctx, &plan).unwrap();
        assert!(f.ctx.game_dir.join("mod.pak").is_file());
    }

    #[test]
    fn dry_run_shows_the_chain_name_not_the_mod_filename() {
        let mut f = fixture();
        f.ctx.pak_chain = Some(wilds_chain());
        stage(&f.ctx, "opt/x.pak", b"P");
        let plan = plan_of(&[("opt/x.pak", "x.pak", 1)]);
        let dr = dry_run(&f.ctx, &plan).unwrap();
        assert_eq!(dr.creates, vec![wilds_chain().filename(1)]);
    }

    /// Every file under `dir`, keyed by path relative to it, so a whole game
    /// directory can be compared before and after in one assertion.
    fn snapshot(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, root: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    let rel = path.strip_prefix(root).unwrap().display().to_string();
                    out.insert(rel, fs::read(&path).unwrap());
                }
            }
        }
        let mut out = std::collections::BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    #[test]
    fn progress_is_reported_once_per_planned_file_and_ends_at_the_totals() {
        let f = fixture();
        for name in ["a", "b", "c"] {
            stage(&f.ctx, &format!("opt/natives/{name}.pak"), b"1234");
        }
        let plan = plan_of(&[
            ("opt/natives/a.pak", "natives/a.pak", 4),
            ("opt/natives/b.pak", "natives/b.pak", 4),
            ("opt/natives/c.pak", "natives/c.pak", 4),
        ]);

        let mut seen: Vec<(usize, u64, String)> = Vec::new();
        let applied = apply_with(&f.ctx, &plan, &mut |p| {
            seen.push((p.files_done, p.bytes_done, p.current.clone()));
            Flow::Continue
        })
        .unwrap();

        assert!(matches!(applied, Applied::Complete(_)));
        assert_eq!(
            seen,
            vec![
                (1, 4, "natives/a.pak".to_string()),
                (2, 8, "natives/b.pak".to_string()),
                (3, 12, "natives/c.pak".to_string()),
            ],
            "one report per file, naming the file just written"
        );

        let mut last = ApplyProgress::default();
        apply_with(&f.ctx, &plan, &mut |p| {
            last = p.clone();
            Flow::Continue
        })
        .unwrap();
        assert_eq!(last.files_done, last.files_total);
        assert_eq!(last.bytes_done, last.bytes_total);
        assert_eq!(last.files_total, 3);
        assert_eq!(last.bytes_total, 12);
    }

    #[test]
    fn cancelling_partway_leaves_the_game_directory_exactly_as_it_was() {
        let f = fixture();
        let vanilla = f.ctx.game_dir.join("natives/shared.pak");
        fs::create_dir_all(vanilla.parent().unwrap()).unwrap();
        fs::write(&vanilla, b"VANILLA").unwrap();
        let before = snapshot(&f.ctx.game_dir);

        stage(&f.ctx, "opt/one.pak", b"ONE");
        stage(&f.ctx, "opt/natives/shared.pak", b"MODDED");
        stage(&f.ctx, "opt/three.pak", b"THREE");
        let plan = plan_of(&[
            ("opt/one.pak", "one.pak", 3),
            ("opt/natives/shared.pak", "natives/shared.pak", 6),
            ("opt/three.pak", "three.pak", 5),
        ]);

        // Cancel once two of the three files are down, so the rollback has both
        // a created file and a vaulted original to undo.
        let mut reports = 0usize;
        let applied = apply_with(&f.ctx, &plan, &mut |p| {
            reports += 1;
            if p.files_done == 2 {
                Flow::Cancel
            } else {
                Flow::Continue
            }
        })
        .unwrap();

        let Applied::Cancelled { journal, rollback } = applied else {
            panic!("expected a cancelled apply");
        };
        assert_eq!(reports, 2, "no further files are placed after the cancel");
        assert!(rollback.is_clean(), "{rollback:?}");
        assert_eq!(
            journal.ops().len(),
            2,
            "only the placed files are journaled"
        );

        assert_eq!(
            fs::read(&vanilla).unwrap(),
            b"VANILLA",
            "the replaced original is back byte-exact"
        );
        assert!(
            !f.ctx.game_dir.join("one.pak").exists(),
            "a file the apply created is gone"
        );
        assert!(!f.ctx.game_dir.join("three.pak").exists());
        assert_eq!(
            snapshot(&f.ctx.game_dir),
            before,
            "the whole game directory is byte-identical to before the apply"
        );
    }

    #[test]
    fn the_journal_records_the_staged_path_each_file_was_placed_from() {
        let f = fixture();
        let vanilla = f.ctx.game_dir.join("natives/shared.pak");
        fs::create_dir_all(vanilla.parent().unwrap()).unwrap();
        fs::write(&vanilla, b"VANILLA").unwrap();
        stage(&f.ctx, "opt/natives/shared.pak", b"MODDED");
        stage(&f.ctx, "opt/deep/new.pak", b"NEW");
        let plan = plan_of(&[
            ("opt/natives/shared.pak", "natives/shared.pak", 6),
            ("opt/deep/new.pak", "natives/new.pak", 3),
        ]);

        let journal = apply(&f.ctx, &plan).unwrap();
        let staged: Vec<(&str, &str)> = journal
            .ops()
            .iter()
            .filter_map(|op| match op {
                JournalOp::Replaced {
                    game_rel_path,
                    staged_rel_path,
                    ..
                }
                | JournalOp::Created {
                    game_rel_path,
                    staged_rel_path,
                    ..
                } => Some((game_rel_path.as_str(), staged_rel_path.as_str())),
                _ => None,
            })
            .collect();

        assert_eq!(
            staged,
            vec![
                ("natives/shared.pak", "opt/natives/shared.pak"),
                ("natives/new.pak", "opt/deep/new.pak"),
            ],
            "the source is recoverable from the journal alone"
        );
    }

    #[test]
    fn invalid_plans_are_refused() {
        let f = fixture();
        let mut plan = plan_of(&[]);
        plan.issues.push(apoc_domain::ValidationIssue {
            message: "missing required option".into(),
            option_ids: vec![],
        });
        assert!(matches!(
            apply(&f.ctx, &plan),
            Err(DeployError::InvalidPlan(_))
        ));
    }
}
