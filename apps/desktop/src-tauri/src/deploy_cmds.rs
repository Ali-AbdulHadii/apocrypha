//! Deployment commands: applying, watching, cancelling, checking and repairing.
//!
//! Everything here that touches the game directory runs on its own thread and
//! reports through events, rather than blocking until it is finished. Applying a
//! large mod set writes thousands of files, and the previous shape (one blocking
//! command that returned a result) meant the interface could only say "linking"
//! and hope. It also meant there was no moment at which a user could change their
//! mind.
//!
//! See the module docs in `commands.rs` for why the `(async)` attribute is on
//! nearly everything.

use crate::commands::{
    build_context, enabled_mod_ids, err, plan_for_profile, profile_of, revert_current,
};
use crate::state::*;
use apoc_deploy::{verify, Applied, ApplyProgress, Flow};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

/// Progress is emitted at most this often. A deployment places files faster than
/// a webview can paint, so emitting per file floods the event channel and makes
/// the window less responsive rather than more informative.
const PROGRESS_INTERVAL_MS: u128 = 100;

/// Start applying every enabled mod. Returns as soon as the work is queued.
///
/// Progress arrives as `deploy-progress` and the single final outcome as
/// `deploy-finished`, so the interface never waits on this call.
#[tauri::command(async)]
pub fn start_deploy(
    app: tauri::AppHandle,
    state: State<AppState>,
    game_id: String,
) -> CmdResult<()> {
    // Claim the slot before any work, so two Apply presses cannot interleave
    // writes to the same game directory.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.deploy_cancel.lock().map_err(|_| "state poisoned")?;
        if slot.is_some() {
            return Err("A deployment is already running.".into());
        }
        *slot = Some(cancel.clone());
    }

    // Planning happens on this thread so a plan that cannot be built is reported
    // as a plain command error, the way it always was, instead of arriving later
    // as an event the caller has to correlate.
    let planned = plan_for_profile(&state, &game_id)
        .and_then(|plan| enabled_mod_ids(&state, &game_id).map(|ids| (plan, ids)))
        .and_then(|(plan, ids)| build_context(&state, &game_id).map(|ctx| (plan, ids, ctx)));

    let (plan, deployed_ids, ctx) = match planned {
        Ok(v) => v,
        Err(e) => {
            release(&state);
            return Err(e);
        }
    };

    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let outcome = {
            // Held for the run, so the slot is freed even if the work panics.
            // Without it a panic would leave the slot claimed and every later
            // Apply would be refused until the app was restarted.
            let _slot = SlotGuard(&state);
            run_deploy(&app, &state, &game_id, plan, deployed_ids, ctx, &cancel)
        };
        let _ = app.emit("deploy-finished", outcome);
    });

    Ok(())
}

/// Releases the in-flight deployment slot when dropped.
struct SlotGuard<'a>(&'a AppState);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        release(self.0);
    }
}

/// Give up the in-flight slot. Called on every exit path, including errors, or
/// a failed deploy would lock out every later attempt until a restart.
fn release(state: &AppState) {
    // `lock` fails only if a previous holder panicked; recovering the guard is
    // right here, because leaving the slot claimed is the worse outcome.
    match state.deploy_cancel.lock() {
        Ok(mut slot) => *slot = None,
        Err(poisoned) => *poisoned.into_inner() = None,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_deploy(
    app: &tauri::AppHandle,
    state: &AppState,
    game_id: &str,
    plan: apoc_domain::DeploymentPlan,
    deployed_ids: Vec<String>,
    ctx: apoc_deploy::DeployContext,
    cancel: &AtomicBool,
) -> DeployOutcomeView {
    let emit = |phase: &str, p: &ApplyProgress| {
        let _ = app.emit(
            "deploy-progress",
            ApplyProgressView {
                phase: phase.to_string(),
                files_done: p.files_done,
                files_total: p.files_total,
                bytes_done: p.bytes_done,
                bytes_total: p.bytes_total,
                current: p.current.clone(),
            },
        );
    };

    // Undoing the previous deployment is real work on a large library, so it is
    // named as its own phase rather than counted as part of applying.
    emit("reverting", &ApplyProgress::default());
    if let Err(e) = revert_current(state, game_id, &ctx) {
        return failed(e);
    }

    let dry = match apoc_deploy::dry_run(&ctx, &plan) {
        Ok(d) => d,
        Err(e) => return failed(err(e)),
    };

    let mut last = std::time::Instant::now();
    let mut sink = |p: &ApplyProgress| {
        // The final file always reports, so the bar cannot finish at 99%.
        if last.elapsed().as_millis() >= PROGRESS_INTERVAL_MS || p.files_done == p.files_total {
            last = std::time::Instant::now();
            emit("linking", p);
        }
        if cancel.load(Ordering::Relaxed) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    };

    match apoc_deploy::apply_with(&ctx, &plan, &mut sink) {
        Err(e) => failed(err(e)),
        Ok(Applied::Cancelled { rollback, .. }) => DeployOutcomeView {
            cancelled: true,
            result: None,
            error: None,
            rollback: Some(RollbackView {
                removed: rollback.removed.len(),
                restored: rollback.restored.len(),
                skipped_modified: rollback.skipped_modified.clone(),
                errors: rollback.errors.clone(),
                clean: rollback.is_clean(),
            }),
        },
        Ok(Applied::Complete(mut journal)) => {
            // The files are down; now the list that points at them. After the
            // apply and against the same journal, so a plugin list written for
            // files that are not there is not a state this can reach.
            let plugin_list = fold_in_plugin_list(state, game_id, &ctx, &mut journal, &plan);

            if let Err(e) = record(state, game_id, &journal, &deployed_ids) {
                return failed(e);
            }
            DeployOutcomeView {
                cancelled: false,
                result: Some(DeployResultView {
                    deployment_id: journal.id().to_string(),
                    files_deployed: plan.file_count(),
                    bytes: plan.total_size(),
                    method: dry.method.as_str().to_string(),
                    plugin_list,
                }),
                error: None,
                rollback: None,
            }
        }
    }
}

/// Write the game's plugin list, for a game that has one.
///
/// Returns `None` when this game is not ordered that way, or when the prefix
/// does not exist yet — a game that has never been launched under Proton has
/// nowhere for the list to go, and the plugins are still installed, so this is
/// not a deployment failure.
///
/// A failure here is reported and does not fail the deploy either. The files
/// are placed and reversible by the time this runs; refusing the whole
/// deployment because a list could not be written would undo work that
/// succeeded in order to report something the user can fix in another tool.
fn fold_in_plugin_list(
    state: &AppState,
    game_id: &str,
    ctx: &apoc_deploy::DeployContext,
    journal: &mut apoc_deploy::journal::Journal,
    plan: &apoc_domain::DeploymentPlan,
) -> Option<PluginListResultView> {
    let profile = crate::commands::effective_profile(state, game_id).ok()?;
    let spec = profile.plugin_list.as_ref()?;
    if !profile.manages_plugin_list() {
        return None;
    }

    let prefix = {
        let store = state.store.lock().ok()?;
        store.get_game(game_id).ok()??.proton_prefix?
    };
    let target = apoc_deploy::plugin_list::PluginListTarget::resolve(Path::new(&prefix), spec)?;

    let paths: Vec<String> = plan.files.iter().map(|f| f.game_rel_path.clone()).collect();
    let rules = crate::commands::rules_for_state(state, game_id);
    let entries = apoc_modengine::plugins::deployed_entries(&ctx.game_dir, &paths, &rules);

    match apoc_deploy::provision_plugin_list(ctx, journal, &target, entries) {
        Ok(outcome) => Some(PluginListResultView {
            added: outcome.added,
            problems: outcome
                .violations
                .iter()
                .map(|v| match v.reason {
                    apoc_domain::plugins::MasterProblem::Missing => format!(
                        "{} needs {}, which is not installed or is switched off.",
                        v.plugin, v.master
                    ),
                    apoc_domain::plugins::MasterProblem::LoadsTooLate => format!(
                        "{} loads before {}, which it depends on. Move {} above it.",
                        v.plugin, v.master, v.master
                    ),
                })
                .collect(),
            written: outcome.written,
        }),
        Err(e) => Some(PluginListResultView {
            added: Vec::new(),
            problems: vec![format!("The plugin list could not be written: {e}")],
            written: false,
        }),
    }
}

fn failed(message: String) -> DeployOutcomeView {
    DeployOutcomeView {
        cancelled: false,
        result: None,
        error: Some(message),
        rollback: None,
    }
}

fn record(
    state: &AppState,
    game_id: &str,
    journal: &apoc_deploy::journal::Journal,
    deployed_ids: &[String],
) -> CmdResult<()> {
    let profile_id = profile_of(state, game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .record_deployment(
            journal.id(),
            game_id,
            Some(profile_id),
            &journal.path().display().to_string(),
            "applied",
        )
        .map_err(err)?;
    store
        .set_deployed_mods(journal.id(), deployed_ids)
        .map_err(err)
}

/// Ask the running deployment to stop. It rolls back what it has written.
///
/// Not an error when nothing is running: the button can be pressed as the last
/// file lands, and reporting that as a failure would be noise.
#[tauri::command]
pub fn cancel_deploy(state: State<AppState>) -> CmdResult<()> {
    let slot = state.deploy_cancel.lock().map_err(|_| "state poisoned")?;
    if let Some(flag) = slot.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/* ------------------------------------------------------------- conflicts -- */

/// Files claimed by more than one enabled mod, and who currently wins each.
///
/// Deliberately plan-only: it does not probe the game directory the way
/// `preview_deploy` does, so the Load order screen can recompute this after every
/// drag without touching the disk.
#[tauri::command(async)]
pub fn list_conflicts(state: State<AppState>, game_id: String) -> CmdResult<Vec<ConflictView>> {
    let plan = match plan_for_profile(&state, &game_id) {
        Ok(p) => p,
        // No enabled mods is a normal state for this screen, not a failure.
        Err(_) => return Ok(Vec::new()),
    };

    // A combined plan reports two kinds of conflict that happen to share a type:
    // one mod's options overwriting each other (contenders are option ids), and
    // two mods claiming the same path (contenders are mod ids). Only the second
    // kind is about load order, and the first would render as unresolvable ids in
    // a list that names mods, so it is filtered out here rather than in the UI.
    let known: std::collections::HashSet<String> = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .list_mods(&game_id)
            .map_err(err)?
            .into_iter()
            .map(|m| m.id)
            .collect()
    };

    Ok(plan
        .conflicts
        .iter()
        .filter(|c| c.contenders.iter().all(|id| known.contains(id)))
        .map(|c| ConflictView {
            path: c.game_rel_path.clone(),
            contenders: c.contenders.clone(),
            winner: c.winner.clone(),
        })
        .collect())
}

/// Pin one contested file to a specific mod, whatever the load order says.
#[tauri::command(async)]
pub fn set_conflict_override(
    state: State<AppState>,
    game_id: String,
    path: String,
    mod_id: String,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .set_conflict_override(profile_id, &path, &mod_id)
        .map_err(err)
}

/// Return a file to whatever the load order decides.
#[tauri::command(async)]
pub fn clear_conflict_override(
    state: State<AppState>,
    game_id: String,
    path: String,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .clear_conflict_override(profile_id, &path)
        .map_err(err)
}

#[tauri::command(async)]
pub fn conflict_overrides(
    state: State<AppState>,
    game_id: String,
) -> CmdResult<std::collections::HashMap<String, String>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.conflict_overrides(profile_id).map_err(err)
}

/* -------------------------------------------------------- verify / repair -- */

fn applied_journals(state: &AppState, game_id: &str) -> CmdResult<Vec<PathBuf>> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(store
        .applied_deployments(game_id)
        .map_err(err)?
        .into_iter()
        .map(|(_, path)| PathBuf::from(path))
        .collect())
}

/// Check that what the journal says is deployed is actually in the game folder.
#[tauri::command(async)]
pub fn verify_deployment(state: State<AppState>, game_id: String) -> CmdResult<VerifyReportView> {
    let ctx = build_context(&state, &game_id)?;
    let journals = applied_journals(&state, &game_id)?;
    if journals.is_empty() {
        return Err("Nothing is deployed, so there is nothing to check.".into());
    }

    let mut checked = 0usize;
    let mut ok = 0usize;
    let mut problems = Vec::new();
    for path in journals {
        let Ok(journal) = apoc_deploy::journal::Journal::load(&path) else {
            return Err(format!(
                "Could not read the change log at {}.",
                path.display()
            ));
        };
        let report = verify::verify(&ctx, &journal);
        checked += report.checked;
        ok += report.ok;
        problems.extend(report.problems.into_iter().map(|p| {
            FileVerdictView {
                path: p.game_rel_path,
                state: match p.state {
                    verify::FileState::Ok => "ok",
                    verify::FileState::Missing => "missing",
                    verify::FileState::Modified => "modified",
                }
                .to_string(),
                repairable: p.repairable,
            }
        }));
    }

    Ok(VerifyReportView {
        checked,
        ok,
        intact: problems.is_empty(),
        problems,
    })
}

/// Put the named files back to what Apocrypha deployed.
///
/// The caller passes the exact paths to act on, because re-placing a file that
/// something else changed overwrites that change, and only the user can decide
/// that is what they want.
#[tauri::command(async)]
pub fn repair_deployment(
    state: State<AppState>,
    game_id: String,
    paths: Vec<String>,
) -> CmdResult<RepairReportView> {
    let ctx = build_context(&state, &game_id)?;
    let journals = applied_journals(&state, &game_id)?;
    let wanted: std::collections::HashSet<&str> = paths.iter().map(String::as_str).collect();

    let mut out = RepairReportView {
        repaired: Vec::new(),
        skipped: Vec::new(),
        errors: Vec::new(),
    };
    for path in journals {
        let Ok(journal) = apoc_deploy::journal::Journal::load(&path) else {
            out.errors.push(format!(
                "Could not read the change log at {}.",
                path.display()
            ));
            continue;
        };
        // Re-verify per journal rather than trusting the paths blindly: the
        // verdict carries the staged source and repairability that repair needs,
        // and the disk may have changed since the report was shown.
        let act_on: Vec<_> = verify::verify(&ctx, &journal)
            .problems
            .into_iter()
            .filter(|p| wanted.contains(p.game_rel_path.as_str()))
            .collect();
        if act_on.is_empty() {
            continue;
        }
        let report = verify::repair(&ctx, &journal, &act_on);
        out.repaired.extend(report.repaired);
        out.skipped.extend(report.skipped);
        out.errors.extend(report.errors);
    }
    Ok(out)
}

/* ----------------------------------------------------------- disk usage --- */

/// Bytes used by everything under `dir`. Missing directories count as zero,
/// which is the honest answer before a game has been set up.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            // Apparent size, not blocks. Hard links and reflinks mean the real
            // cost is usually far lower, but a user reading this wants to know
            // how much of their library this folder accounts for, which is the
            // number they would also see in a file manager.
            total += meta.len();
        }
    }
    total
}

/// What each folder Apocrypha owns currently costs on disk.
///
/// Walks the tree, so it is slow on a large library and must never run on the
/// main thread.
#[tauri::command(async)]
pub fn storage_usage(state: State<AppState>) -> CmdResult<StorageUsageView> {
    let root = state.paths.root().to_path_buf();
    let downloads = state.downloads_dir();

    // Only games the user has actually configured: an unconfigured game has no
    // folders, and listing it at 0 B reads as a problem rather than as absence.
    let game_ids: Vec<String> = {
        use apoc_gamedef::GameDatabaseSource;
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        apoc_gamedef::LocalBuiltin::new()
            .all()
            .map_err(err)?
            .into_iter()
            .map(|p| p.id)
            .filter(|id| store.get_game(id).ok().flatten().is_some())
            .collect()
    };

    // Each of these three lives under a per-game folder, so with one game
    // configured the real path can be named. With several there is no single
    // path to open, and pointing three different rows at the same shared
    // directory would be worse than saying it covers every game.
    let one_game = if game_ids.len() == 1 {
        game_ids.first().cloned()
    } else {
        None
    };
    let shared = root.join("games").display().to_string();
    let describe = |f: &dyn Fn(&str) -> PathBuf, hint: &str| -> (String, u64, String) {
        let bytes = game_ids.iter().map(|id| dir_size(&f(id))).sum();
        match &one_game {
            Some(id) => (f(id).display().to_string(), bytes, hint.to_string()),
            None => (
                shared.clone(),
                bytes,
                format!("{hint} Added up across every game you have set up."),
            ),
        }
    };

    let (lib_path, lib_bytes, lib_hint) = describe(
        &|id| state.paths.staging_root(id),
        "Apocrypha's own copy of every mod you have added.",
    );
    let (vault_path, vault_bytes, vault_hint) = describe(
        &|id| state.paths.vault(id),
        "Original game files that mods replaced. Undo needs these.",
    );
    let (log_path, log_bytes, log_hint) = describe(
        &|id| state.paths.journal(id),
        "A record of every file Apocrypha wrote, so it can be undone.",
    );

    let entries = vec![
        UsageEntryView {
            label: "Mod library".into(),
            path: lib_path,
            bytes: lib_bytes,
            hint: lib_hint,
        },
        UsageEntryView {
            label: "Backups".into(),
            path: vault_path,
            bytes: vault_bytes,
            hint: vault_hint,
        },
        UsageEntryView {
            label: "Change log".into(),
            path: log_path,
            bytes: log_bytes,
            hint: log_hint,
        },
        UsageEntryView {
            label: "Downloads".into(),
            path: downloads.display().to_string(),
            bytes: dir_size(&downloads),
            hint: "Archives you downloaded. Safe to delete once a mod is added.".into(),
        },
    ];

    Ok(StorageUsageView {
        total: entries.iter().map(|e| e.bytes).sum(),
        entries,
    })
}

/// Show a folder in the user's file manager.
#[tauri::command(async)]
pub fn open_path(app: tauri::AppHandle, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = PathBuf::from(&path);
    if !dir.exists() {
        return Err(format!("{path} does not exist yet."));
    }
    app.opener().open_path(path, None::<&str>).map_err(err)
}
