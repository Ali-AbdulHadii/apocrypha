//! Tauri command surface: the entire API the React UI can call.
//!
//! Every command is a thin adapter: validate input, call an engine, map the
//! result into a view DTO. No mod-management logic lives here.
//!
//! # Why the `(async)` on most of these
//!
//! A plain `#[tauri::command]` on a non-async function runs **on the main
//! thread**. On Linux that is the GTK loop driving the webview, so a command
//! that blocks for a second stops the window repainting and the desktop puts up
//! an "is not responding" dialog over a perfectly healthy application. Staging a
//! few hundred files, hashing an archive, or waiting on a Nexus request all take
//! long enough to trigger it.
//!
//! `#[tauri::command(async)]` on a synchronous function moves it to a worker
//! thread instead. The rule here is: anything that touches the disk, the
//! network, or spawns a process gets it. What is left is a handful of
//! single-row sqlite calls and pure computation, where the thread hop would cost
//! more than the work.
//!
//! `AppState` holds its `Store` behind a `Mutex`, so commands running
//! concurrently is safe. If you add a command that does real work, add the
//! `(async)` with it.

use crate::state::*;
use apoc_deploy::{place::Ladder, DeployContext};
use apoc_domain::GameProfile;
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_storage::{GameDbSource, GameRecord, ModRecord, ModState};
use std::path::{Path, PathBuf};
use tauri::State;

type CmdResult<T> = Result<T, String>;

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub(crate) fn profile_of(state: &AppState, game_id: &str) -> CmdResult<i64> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let game = store.get_game(game_id).map_err(err)?;
    if let Some(id) = game.and_then(|g| g.active_profile_id) {
        return Ok(id);
    }
    let id = store.ensure_profile(game_id, "Default").map_err(err)?;
    store.set_active_profile(game_id, id).map_err(err)?;
    Ok(id)
}

fn builtin_profile(game_id: &str) -> CmdResult<GameProfile> {
    LocalBuiltin::new().get(game_id).map_err(err)
}

/// Payload-recognition rules for a game, so loader DLLs and engine-specific
/// payload roots are recognized. Falls back to defaults for an unknown game.
fn rules_for(game_id: &str) -> apoc_modengine::GameRules {
    builtin_profile(game_id)
        .map(|p| apoc_modengine::GameRules::from_profile(&p))
        .unwrap_or_default()
}

/// List all known games, merged with detection and stored configuration.
#[tauri::command(async)]
pub fn list_games(state: State<AppState>) -> CmdResult<Vec<GameView>> {
    let profiles = LocalBuiltin::new().all().map_err(err)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;

    let mut out = Vec::new();
    for p in profiles {
        let stored = store.get_game(&p.id).map_err(err)?;
        let detected = apoc_steam::find_game(p.detection.steam_app_id);

        let install_dir = stored
            .as_ref()
            .and_then(|g| g.install_dir.clone())
            .or_else(|| {
                detected
                    .as_ref()
                    .map(|d| d.install_dir.display().to_string())
            });
        let proton_prefix = stored
            .as_ref()
            .and_then(|g| g.proton_prefix.clone())
            .or_else(|| {
                detected
                    .as_ref()
                    .and_then(|d| d.proton_prefix.as_ref())
                    .map(|p| p.display().to_string())
            });

        let loader = p.loader.as_ref();
        let user_reg = proton_prefix.as_ref().map(|p| PathBuf::from(p).join("user.reg"));
        // "Ready" means every override the loader declares is registered, not
        // just the first: Cyberpunk needs both RED4ext's and CET's, and half a
        // stack reported as ready is worse than reported as missing.
        let loader_override_active = match (loader, &user_reg) {
            (Some(l), Some(reg)) => {
                let wanted = l.dll_overrides();
                !wanted.is_empty()
                    && wanted.iter().all(|(name, _)| {
                        apoc_deploy::loader::read_override(reg, name)
                            .map(|s| s.value.is_some())
                            .unwrap_or(false)
                    })
            }
            _ => false,
        };

        out.push(GameView {
            id: p.id.clone(),
            name: p.name.clone(),
            engine: format!("{:?}", p.engine),
            steam_app_id: p.detection.steam_app_id,
            load_order: format!("{:?}", p.load_order),
            detected: install_dir.is_some(),
            install_dir,
            proton_prefix,
            proton_tool: detected.as_ref().and_then(|d| d.proton_tool.clone()),
            loader_name: loader.map(|l| l.name.clone()),
            loader_dll: loader.and_then(|l| l.proxy_dll.clone()),
            loader_override_active,
            steam_launch_options: loader.and_then(|l| l.proton.steam_launch_options.clone()),
            nexus_domain: p.nexus_domain.clone(),
        });
    }
    Ok(out)
}

/// The game a Nexus domain belongs to, if any is known.
///
/// An `nxm://` link names the game it came from, and now that more than one
/// game ships, "download to whatever is on screen" would quietly file a
/// Cyberpunk mod under Monster Hunter. Matching is case-insensitive because
/// the domain arrives from a URL.
#[tauri::command]
pub fn game_for_domain(domain: String) -> CmdResult<Option<String>> {
    let profiles = LocalBuiltin::new().all().map_err(err)?;
    Ok(profiles
        .into_iter()
        .find(|p| {
            p.nexus_domain
                .as_deref()
                .is_some_and(|d| d.eq_ignore_ascii_case(domain.trim()))
        })
        .map(|p| p.id))
}

/// Re-run Steam/Proton detection for one game and persist what was found.
#[tauri::command(async)]
pub fn detect_game(state: State<AppState>, game_id: String) -> CmdResult<GameView> {
    let p = builtin_profile(&game_id)?;
    let detected = apoc_steam::find_game(p.detection.steam_app_id);
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .upsert_game(&GameRecord {
                id: p.id.clone(),
                name: p.name.clone(),
                install_dir: detected.as_ref().map(|d| d.install_dir.display().to_string()),
                proton_prefix: detected
                    .as_ref()
                    .and_then(|d| d.proton_prefix.as_ref())
                    .map(|p| p.display().to_string()),
                active_profile_id: None,
            })
            .map_err(err)?;
    }
    state.paths.ensure_game_dirs(&game_id).map_err(err)?;
    list_games(state)?
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "game disappeared after detection".to_string())
}

/// Manually point a game at an install directory (custom/non-Steam installs).
#[tauri::command(async)]
pub fn set_game_path(
    state: State<AppState>,
    game_id: String,
    install_dir: String,
    proton_prefix: Option<String>,
) -> CmdResult<GameView> {
    let dir = PathBuf::from(&install_dir);
    if !dir.is_dir() {
        return Err(format!("not a directory: {install_dir}"));
    }
    let p = builtin_profile(&game_id)?;
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .upsert_game(&GameRecord {
                id: p.id.clone(),
                name: p.name.clone(),
                install_dir: Some(install_dir),
                proton_prefix,
                active_profile_id: None,
            })
            .map_err(err)?;
    }
    state.paths.ensure_game_dirs(&game_id).map_err(err)?;
    list_games(state)?
        .into_iter()
        .find(|g| g.id == game_id)
        .ok_or_else(|| "game not found".to_string())
}

/// Analyze an archive without importing it: powers the wizard preview.
#[tauri::command(async)]
pub fn analyze_archive(game_id: String, path: String) -> CmdResult<ModView> {
    let bundle =
        apoc_modengine::analyze_archive_with(Path::new(&path), &rules_for(&game_id)).map_err(err)?;
    let selection = apoc_modengine::default_selection(&bundle);
    Ok(ModView {
        id: String::new(),
        name: bundle.name.clone(),
        version: bundle.version.clone(),
        author: bundle.author.clone(),
        category: bundle.category.clone(),
        installer_model: format!("{:?}", bundle.installer_model),
        enabled: false,
        priority: 0,
        applied: false,
        added_at: 0,
        groups: groups_view(&bundle),
        selection: selection_vec(&selection),
        total_files: bundle.deployable_options().map(|o| o.payload.len()).sum(),
        total_bytes: bundle.deployable_options().map(|o| o.total_size()).sum(),
    })
}

/// Import a mod: analyze, stage its payloads, and register it in the active profile.
#[tauri::command(async)]
pub fn import_mod(
    state: State<AppState>,
    game_id: String,
    archive_path: String,
    selection: Vec<String>,
) -> CmdResult<ModView> {
    let path = PathBuf::from(&archive_path);
    let bundle =
        apoc_modengine::analyze_archive_with(&path, &rules_for(&game_id)).map_err(err)?;

    let mod_id = format!(
        "mod-{}",
        bundle
            .archive_sha256
            .as_deref()
            .map(|h| &h[..16])
            .unwrap_or("unknown")
    );

    state.paths.ensure_game_dirs(&game_id).map_err(err)?;
    let staging = staging_for(&state.paths, &game_id, &mod_id);
    apoc_modengine::stage_bundle(&path, &bundle, &staging).map_err(err)?;

    let sel = if selection.is_empty() {
        apoc_modengine::default_selection(&bundle)
    } else {
        selection_from(&selection)
    };

    let profile_id = profile_of(&state, &game_id)?;
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .insert_mod(&ModRecord {
                id: mod_id.clone(),
                game_id: game_id.clone(),
                name: bundle.name.clone(),
                version: bundle.version.clone(),
                author: bundle.author.clone(),
                archive_path,
                archive_sha256: bundle.archive_sha256.clone(),
                installer_model: format!("{:?}", bundle.installer_model),
                imported_at: 0,
                // Set by the Nexus import path, which knows the ids the file was
                // fetched under. A mod added from disk has no Nexus identity, so
                // an update check has nothing to ask about, which is correct.
                nexus_mod_id: None,
                nexus_file_id: None,
                bundle: bundle.clone(),
            })
            .map_err(err)?;
        store
            .set_mod_state(
                profile_id,
                &ModState {
                    mod_id: mod_id.clone(),
                    enabled: true,
                    priority: 0,
                    selection: sel.clone(),
                },
            )
            .map_err(err)?;
    }

    Ok(ModView {
        id: mod_id,
        name: bundle.name.clone(),
        version: bundle.version.clone(),
        author: bundle.author.clone(),
        category: bundle.category.clone(),
        installer_model: format!("{:?}", bundle.installer_model),
        enabled: true,
        priority: 0,
        applied: false,
        added_at: 0,
        groups: groups_view(&bundle),
        selection: selection_vec(&sel),
        total_files: bundle.deployable_options().map(|o| o.payload.len()).sum(),
        total_bytes: bundle.deployable_options().map(|o| o.total_size()).sum(),
    })
}

/// List installed mods for a game, with their state in the active profile.
#[tauri::command(async)]
pub fn list_mods(state: State<AppState>, game_id: String) -> CmdResult<Vec<ModView>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let mods = store.list_mods(&game_id).map_err(err)?;
    let applied = store.applied_mod_ids(&game_id).map_err(err)?;

    let mut out = Vec::new();
    for m in mods {
        let st = store.get_mod_state(profile_id, &m.id).map_err(err)?;
        let sel = st
            .as_ref()
            .map(|s| s.selection.clone())
            .unwrap_or_else(|| apoc_modengine::default_selection(&m.bundle));
        out.push(ModView {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            author: m.author.clone(),
            category: m.bundle.category.clone(),
            installer_model: m.installer_model.clone(),
            enabled: st.as_ref().map(|s| s.enabled).unwrap_or(false),
            priority: st.as_ref().map(|s| s.priority).unwrap_or(0),
            applied: applied.contains(&m.id),
            added_at: m.imported_at,
            groups: groups_view(&m.bundle),
            selection: selection_vec(&sel),
            total_files: m.bundle.deployable_options().map(|o| o.payload.len()).sum(),
            total_bytes: m.bundle.deployable_options().map(|o| o.total_size()).sum(),
        });
    }
    Ok(out)
}

/// Enable or disable a mod without deleting anything.
#[tauri::command]
pub fn set_mod_enabled(
    state: State<AppState>,
    game_id: String,
    mod_id: String,
    enabled: bool,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.set_enabled(profile_id, &mod_id, enabled).map_err(err)
}

/// Persist a new wizard selection for a mod (radio semantics applied server-side).
#[tauri::command]
pub fn set_mod_selection(
    state: State<AppState>,
    game_id: String,
    mod_id: String,
    selection: Vec<String>,
) -> CmdResult<Vec<String>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let m = store
        .get_mod(&mod_id)
        .map_err(err)?
        .ok_or_else(|| format!("unknown mod {mod_id}"))?;

    // Re-apply exclusivity so the stored selection is always internally valid.
    let mut sel = apoc_domain::Selection::new();
    for id in &selection {
        let Some(opt) = m.bundle.options().find(|o| &o.id == id) else {
            continue;
        };
        match opt.select_mode {
            apoc_domain::SelectMode::Info => {}
            apoc_domain::SelectMode::Exclusive => {
                apoc_modengine::choose_exclusive(&m.bundle, &mut sel, id)
            }
            _ => sel.insert(id.clone()),
        }
    }
    for o in m.bundle.options() {
        if o.select_mode == apoc_domain::SelectMode::Forced && o.deployable {
            sel.insert(o.id.clone());
        }
    }

    let existing = store.get_mod_state(profile_id, &mod_id).map_err(err)?;
    store
        .set_mod_state(
            profile_id,
            &ModState {
                mod_id: mod_id.clone(),
                enabled: existing.as_ref().map(|s| s.enabled).unwrap_or(true),
                priority: existing.as_ref().map(|s| s.priority).unwrap_or(0),
                selection: sel.clone(),
            },
        )
        .map_err(err)?;
    Ok(selection_vec(&sel))
}

/// Build the deployment context for a game.
///
/// `staging_dir` is the game's shared staging root, not one mod's folder, so a
/// single deployment can span every enabled mod.
pub(crate) fn build_context(state: &AppState, game_id: &str) -> CmdResult<DeployContext> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let game = store
        .get_game(game_id)
        .map_err(err)?
        .ok_or_else(|| "game not configured: run detection first".to_string())?;
    let install_dir = game
        .install_dir
        .ok_or_else(|| "game install directory unknown".to_string())?;
    Ok(DeployContext {
        game_id: game_id.to_string(),
        game_dir: PathBuf::from(install_dir),
        staging_dir: state.paths.staging_root(game_id),
        vault_dir: state.paths.vault(game_id),
        journal_dir: state.paths.journal(game_id),
        ladder: Ladder::default(),
        pak_chain: builtin_profile(game_id).ok().and_then(|p| p.pak_chain),
    })
}

/// Build one combined plan covering **every enabled mod** in the active profile,
/// ordered by load-order priority. Deploying a single mod at a time was the
/// reason a modded game could launch with most mods missing.
pub(crate) fn plan_for_profile(
    state: &AppState,
    game_id: &str,
) -> CmdResult<apoc_domain::DeploymentPlan> {
    let profile_id = profile_of(state, game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let mods = store.list_mods(game_id).map_err(err)?;
    // Per-file winners the user pinned. Applied inside the planner rather than
    // after it, so the reported conflict winner and the file actually planned
    // can never disagree.
    let overrides = store.conflict_overrides(profile_id).map_err(err)?;

    let mut parts = Vec::new();
    for m in mods {
        let Some(st) = store.get_mod_state(profile_id, &m.id).map_err(err)? else {
            continue;
        };
        if !st.enabled {
            continue;
        }
        let plan = apoc_modengine::plan_deployment(&m.bundle, &st.selection);
        if plan.files.is_empty() && plan.issues.is_empty() {
            continue;
        }
        parts.push(apoc_modengine::ModPlan {
            mod_id: m.id.clone(),
            priority: st.priority,
            plan,
        });
    }

    if parts.is_empty() {
        return Err("no enabled mods to deploy".to_string());
    }
    let label = if parts.len() == 1 {
        parts[0].plan.bundle_name.clone()
    } else {
        format!("{} mods", parts.len())
    };
    Ok(apoc_modengine::combine_plans_with_overrides(
        parts, &label, &overrides,
    ))
}

/// Roll back whatever is currently applied, so an Apply always reconciles the
/// game directory to exactly the current profile rather than layering onto a
/// previous deployment.
pub(crate) fn revert_current(state: &AppState, game_id: &str, ctx: &DeployContext) -> CmdResult<()> {
    let outstanding = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.applied_deployments(game_id).map_err(err)?
    };

    for (dep_id, journal_path) in outstanding {
        if let Ok(journal) = apoc_deploy::journal::Journal::load(Path::new(&journal_path)) {
            // The loader override is left in place; only files are reconciled here.
            let _ = apoc_deploy::rollback(ctx, &journal, None);
        }
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .record_deployment(&dep_id, game_id, None, &journal_path, "reverted")
            .map_err(err)?;
    }
    Ok(())
}

/// Preview the deployment of every enabled mod: what would be created,
/// replaced, or is missing.
#[tauri::command(async)]
pub fn preview_deploy(state: State<AppState>, game_id: String) -> CmdResult<DryRunView> {
    let plan = plan_for_profile(&state, &game_id)?;
    let ctx = build_context(&state, &game_id)?;
    let dr = apoc_deploy::dry_run(&ctx, &plan).map_err(err)?;
    Ok(DryRunView {
        method: dr.method.as_str().to_string(),
        file_count: dr.file_count(),
        creates: dr.creates,
        replaces: dr.replaces,
        missing: dr.missing,
        total_bytes: dr.total_bytes,
        conflicts: plan
            .conflicts
            .iter()
            .map(|c| ConflictView {
                path: c.game_rel_path.clone(),
                contenders: c.contenders.clone(),
                winner: c.winner.clone(),
            })
            .collect(),
        issues: plan.issues.iter().map(|i| i.message.clone()).collect(),
    })
}

/// Remove a mod from the library and delete its staged files.
///
/// Refuses while the mod's files are still in the game folder. Deleting the
/// staging copy first would leave those files orphaned with nothing left to
/// remove them, so the user is asked to undo the deployment first.
#[tauri::command(async)]
pub fn remove_mod(state: State<AppState>, game_id: String, mod_id: String) -> CmdResult<()> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        if store
            .applied_mod_ids(&game_id)
            .map_err(err)?
            .contains(&mod_id)
        {
            return Err(
                "This mod's files are still in the game. Use Undo all first, then remove it."
                    .to_string(),
            );
        }
    }

    // Delete the staged payload before the record, so a failure here leaves the
    // mod visible and removable rather than orphaning files with no owner.
    let staging = staging_for(&state.paths, &game_id, &mod_id);
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(err)?;
    }

    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.delete_mod(&mod_id).map_err(err)
}

/// Set load order for the active profile from an ordered list of mod ids.
#[tauri::command]
pub fn set_mod_order(
    state: State<AppState>,
    game_id: String,
    ordered_ids: Vec<String>,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.set_mod_order(profile_id, &ordered_ids).map_err(err)
}

/// Ids of the enabled mods, in load order, that a deployment would cover.
pub(crate) fn enabled_mod_ids(state: &AppState, game_id: &str) -> CmdResult<Vec<String>> {
    let profile_id = profile_of(state, game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(store
        .list_mod_states(profile_id)
        .map_err(err)?
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.mod_id)
        .collect())
}

/// Undeploy: revert every outstanding deployment, returning the game directory
/// to its unmodded state.
#[tauri::command(async)]
pub fn rollback_last(state: State<AppState>, game_id: String) -> CmdResult<RollbackView> {
    let outstanding = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.applied_deployments(&game_id).map_err(err)?
    };
    if outstanding.is_empty() {
        return Err("no deployment to roll back".to_string());
    }

    let (game_dir, proton_prefix) = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        let g = store
            .get_game(&game_id)
            .map_err(err)?
            .ok_or_else(|| "game not configured".to_string())?;
        (
            g.install_dir.ok_or_else(|| "install dir unknown".to_string())?,
            g.proton_prefix,
        )
    };

    let ctx = DeployContext {
        game_id: game_id.clone(),
        game_dir: PathBuf::from(game_dir),
        staging_dir: state.paths.staging_root(&game_id),
        vault_dir: state.paths.vault(&game_id),
        journal_dir: state.paths.journal(&game_id),
        ladder: Ladder::default(),
        pak_chain: builtin_profile(&game_id).ok().and_then(|p| p.pak_chain),
    };
    let user_reg = proton_prefix.map(|p| PathBuf::from(p).join("user.reg"));

    // Newest first, so a file replaced across several deployments is restored
    // back through the same chain it was overwritten along.
    let mut removed = 0usize;
    let mut restored = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (dep_id, journal_path) in &outstanding {
        let Ok(journal) = apoc_deploy::journal::Journal::load(Path::new(journal_path)) else {
            errors.push(format!("unreadable journal {journal_path}"));
            continue;
        };
        let report = apoc_deploy::rollback(&ctx, &journal, user_reg.as_deref());
        removed += report.removed.len();
        restored += report.restored.len();
        skipped.extend(report.skipped_modified.clone());
        errors.extend(report.errors.clone());

        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .record_deployment(dep_id, &game_id, None, journal_path, "reverted")
            .map_err(err)?;
    }

    Ok(RollbackView {
        removed,
        restored,
        skipped_modified: skipped.clone(),
        errors: errors.clone(),
        clean: errors.is_empty() && skipped.is_empty(),
    })
}

/// Register the loader's DLL override in the Proton prefix.
#[tauri::command(async)]
pub fn setup_loader(state: State<AppState>, game_id: String) -> CmdResult<String> {
    if apoc_deploy::loader::steam_is_running() {
        return Err("Close Steam before changing the Proton prefix.".into());
    }
    let profile = builtin_profile(&game_id)?;
    let loader = profile
        .loader
        .as_ref()
        .ok_or_else(|| "this game needs no loader".to_string())?;
    // A game can need several proxies registered (RED4ext and Cyber Engine
    // Tweaks on Cyberpunk). Fall back to the proxy DLL's own name when the
    // profile declares no override string.
    let mut overrides = loader.dll_overrides();
    if overrides.is_empty() {
        let stem = loader
            .proxy_dll_stem()
            .ok_or_else(|| "loader defines no proxy DLL".to_string())?;
        overrides.push((stem.to_string(), "native,builtin".to_string()));
    }

    let prefix = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .get_game(&game_id)
            .map_err(err)?
            .and_then(|g| g.proton_prefix)
            .ok_or_else(|| "Proton prefix not found: run the game once via Steam first.".to_string())?
    };
    let user_reg = PathBuf::from(prefix).join("user.reg");
    let mut written = Vec::new();
    for (name, value) in &overrides {
        apoc_deploy::loader::write_override(&user_reg, name, value).map_err(err)?;
        written.push(format!("{name}={value}"));
    }
    Ok(format!(
        "Registered '{}' in the Proton prefix.",
        written.join("', '")
    ))
}

/// Read the persisted settings.
#[tauri::command]
pub fn get_settings(state: State<AppState>) -> CmdResult<SettingsView> {
    // Resolved before the lock is taken, because it takes the lock itself.
    let downloads = state.downloads_dir();
    let is_default = downloads == state.default_downloads_dir();

    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(SettingsView {
        game_db_source: store.game_db_source().map_err(err)?.as_str().to_string(),
        data_root: state.paths.root().display().to_string(),
        deploy_method_preference: "adaptive".to_string(),
        downloads_dir: downloads.display().to_string(),
        downloads_dir_is_default: is_default,
    })
}

/// Choose where downloads are kept. An empty path restores the default.
///
/// Files already downloaded are left where they are rather than moved. Moving
/// tens of gigabytes as a side effect of a settings change is not something to
/// do without asking, and the new folder is scanned on arrival anyway, so
/// pointing this at an existing collection is how you adopt one.
#[tauri::command(async)]
pub fn set_downloads_dir(state: State<AppState>, path: String) -> CmdResult<SettingsView> {
    let trimmed = path.trim().to_string();

    if !trimmed.is_empty() {
        let dir = PathBuf::from(&trimmed);
        if !dir.is_absolute() {
            return Err("Choose a full path for the downloads folder.".into());
        }
        // Fail here, where the message can name the folder, rather than at the
        // start of a download the user has already committed to.
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not use {}: {e}", dir.display()))?;
        let probe = dir.join(".apocrypha-write-test");
        std::fs::write(&probe, b"")
            .map_err(|e| format!("Apocrypha cannot write to {}: {e}", dir.display()))?;
        let _ = std::fs::remove_file(&probe);
    }

    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_setting(crate::state::KEY_DOWNLOADS_DIR, &trimmed)
            .map_err(err)?;
    }
    get_settings(state)
}

/// Switch the "Game Database Source" between the built-in DB and the Apocrypha API.
#[tauri::command]
pub fn set_game_db_source(state: State<AppState>, source: String) -> CmdResult<SettingsView> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .set_game_db_source(GameDbSource::parse(&source))
            .map_err(err)?;
    }
    get_settings(state)
}

/// List profiles for a game.
#[tauri::command]
pub fn list_profiles(state: State<AppState>, game_id: String) -> CmdResult<Vec<ProfileView>> {
    let active = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(store
        .list_profiles(&game_id)
        .map_err(err)?
        .into_iter()
        .map(|p| ProfileView {
            id: p.id,
            name: p.name,
            active: p.id == active,
        })
        .collect())
}

#[tauri::command]
pub fn create_profile(
    state: State<AppState>,
    game_id: String,
    name: String,
) -> CmdResult<Vec<ProfileView>> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.ensure_profile(&game_id, &name).map_err(err)?;
    }
    list_profiles(state, game_id)
}

#[tauri::command]
pub fn switch_profile(
    state: State<AppState>,
    game_id: String,
    profile_id: i64,
) -> CmdResult<Vec<ProfileView>> {
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.set_active_profile(&game_id, profile_id).map_err(err)?;
    }
    list_profiles(state, game_id)
}

/// Copy a profile, including which mods are on, their options and their order.
///
/// The usual reason to want a second profile is to try something without
/// losing a setup that works, and building the working one again by hand is
/// exactly what the user is trying to avoid.
#[tauri::command(async)]
pub fn duplicate_profile(
    state: State<AppState>,
    game_id: String,
    profile_id: i64,
    name: String,
) -> CmdResult<Vec<ProfileView>> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the copy a name.".into());
    }
    {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        let owned = store
            .list_profiles(&game_id)
            .map_err(err)?
            .into_iter()
            .any(|p| p.id == profile_id);
        if !owned {
            return Err("That profile belongs to a different game.".into());
        }
        store.clone_profile(profile_id, &name).map_err(err)?;
    }
    list_profiles(state, game_id)
}

/// Delete a profile.
///
/// The active profile is refused rather than silently reassigned: the game
/// folder holds what that profile deployed, so removing it out from under the
/// deployment would leave files nothing accounts for. Switch first, then
/// delete.
#[tauri::command(async)]
pub fn delete_profile(
    state: State<AppState>,
    game_id: String,
    profile_id: i64,
) -> CmdResult<Vec<ProfileView>> {
    {
        let active = profile_of(&state, &game_id)?;
        if active == profile_id {
            return Err("That profile is in use. Switch to another one first.".into());
        }
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        let profiles = store.list_profiles(&game_id).map_err(err)?;
        if !profiles.iter().any(|p| p.id == profile_id) {
            return Err("That profile belongs to a different game.".into());
        }
        if profiles.len() <= 1 {
            return Err("A game keeps at least one profile.".into());
        }
        store.delete_profile(profile_id).map_err(err)?;
    }
    list_profiles(state, game_id)
}

/// Encode image bytes as a `data:` URI the webview can render directly.
///
/// Images are served this way rather than over the asset protocol because they
/// live inside archives or the private staging library, neither of which should
/// be exposed to the frontend as a browsable filesystem scope.
fn to_data_uri(bytes: &[u8], filename: &str) -> String {
    let mime = match filename.rsplit('.').next().map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "image/jpeg",
    };
    format!("data:{};base64,{}", mime, base64_encode(bytes))
}

/// Minimal base64 encoder: avoids pulling a dependency for one call site.
fn base64_encode(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Preview image for one option of a not-yet-imported archive (import wizard).
#[tauri::command(async)]
pub fn preview_from_archive(
    game_id: String,
    archive_path: String,
    option_id: String,
) -> CmdResult<Option<String>> {
    let path = PathBuf::from(&archive_path);
    let bundle = apoc_modengine::analyze_archive_no_hash_with(&path, &rules_for(&game_id))
        .map_err(err)?;
    let Some(opt) = bundle.options().find(|o| o.id == option_id) else {
        return Ok(None);
    };
    let (Some(shot), Some(entry)) = (&opt.screenshot, &opt.screenshot_archive_path) else {
        return Ok(None);
    };
    let bytes = apoc_modengine::read_archive_entry(&path, entry).map_err(err)?;
    Ok(Some(to_data_uri(&bytes, shot)))
}

/// Preview image for one option of an imported mod (served from staging).
#[tauri::command(async)]
pub fn preview_from_mod(
    state: State<AppState>,
    game_id: String,
    mod_id: String,
    option_id: String,
) -> CmdResult<Option<String>> {
    let m = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.get_mod(&mod_id).map_err(err)?
    };
    let Some(m) = m else { return Ok(None) };
    let Some(opt) = m.bundle.options().find(|o| o.id == option_id) else {
        return Ok(None);
    };
    let Some(shot) = &opt.screenshot else {
        return Ok(None);
    };

    let staging = staging_for(&state.paths, &game_id, &mod_id);
    if let Some(p) = apoc_modengine::staged_preview(&staging, &option_id, shot) {
        let bytes = std::fs::read(&p).map_err(err)?;
        return Ok(Some(to_data_uri(&bytes, shot)));
    }
    // Older imports predate preview staging: fall back to the source archive.
    if let Some(entry) = &opt.screenshot_archive_path {
        let archive = PathBuf::from(&m.archive_path);
        if archive.is_file() {
            let bytes = apoc_modengine::read_archive_entry(&archive, entry).map_err(err)?;
            return Ok(Some(to_data_uri(&bytes, shot)));
        }
    }
    Ok(None)
}

/// Steam roots and libraries found on this machine (diagnostics panel).
#[tauri::command(async)]
pub fn steam_diagnostics() -> CmdResult<serde_json::Value> {
    let roots: Vec<_> = apoc_steam::discover_steam_roots()
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "path": r.path.display().to_string(),
                "flavor": r.flavor.as_str(),
            })
        })
        .collect();
    let libs: Vec<_> = apoc_steam::discover_libraries()
        .into_iter()
        .map(|l| {
            serde_json::json!({
                "path": l.path.display().to_string(),
                "apps": l.apps.len(),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "roots": roots,
        "libraries": libs,
        "steamRunning": apoc_deploy::loader::steam_is_running(),
    }))
}
