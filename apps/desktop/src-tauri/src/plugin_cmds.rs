//! Reading and editing a game's plugin order.
//!
//! The deploy path already writes this list: it folds newly installed plugins
//! in, switches them on, and reports what it added. What it deliberately never
//! does is *move* anything, because appending is the only change a deploy is
//! entitled to make to an order somebody curated. That left `PluginOrder::move_to`
//! written, correct, tested, and reachable from nowhere — the application could
//! show a user that two plugins load in the wrong order and offer no way to fix
//! it. These commands are the way.
//!
//! **Every edit reaches the file immediately.** There is no staged state and no
//! Save button. `plugins.txt` is what the game reads, so a screen that disagreed
//! with it would be a second source of truth about a file three other tools also
//! write. Each command re-reads from disk rather than trusting anything cached:
//! between two clicks the user may have opened their own load-order tool, and
//! the list on disk is the one that is true.
//!
//! **An edit is journaled, and is not a deployment.** The three invariants hold
//! exactly as they do for a deploy — the previous list is vaulted before a byte
//! is written, the write is journaled and flushed before it counts, and rollback
//! re-hashes before restoring. What an edit is *not* is a row in the deployments
//! table. That table is what "roll back the last deployment" reads, and a plugin
//! reorder landing in it would mean that pressing undo after arranging the list
//! undoes the arranging rather than the mod install the user meant. The journal
//! is written for evidence and reversibility; the deployment history keeps
//! meaning what it has always meant.

use crate::commands::{effective_profile, err, rules_for_state};
use crate::state::*;
use apoc_deploy::journal::{DeploymentHeader, Journal};
use apoc_deploy::plugin_list::{self, PluginListTarget};
use apoc_domain::plugins::{MasterProblem, MasterViolation, PluginOrder};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

type CmdResult<T> = Result<T, String>;

/// One plugin, as the screen draws it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntryView {
    /// The file name the game reads, e.g. `MyMod.esp`.
    pub name: String,
    pub enabled: bool,
    /// `master`, `light-master` or `normal`, from the file's own flags.
    pub kind: String,
    /// The plugins this one depends on, in header order.
    pub masters: Vec<String>,
    /// Loaded by the engine whether the list names it or not. Not movable, and
    /// not switchable off.
    pub implicit: bool,
    /// Whether the file was found where the game would look for it. False for a
    /// line naming a plugin whose mod is no longer installed, which the game
    /// ignores and the user should be able to see and remove.
    pub present: bool,
}

/// A plugin loading before something it depends on, with the move that fixes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationView {
    pub plugin: String,
    pub master: String,
    /// The sentence to show, worded the same way the Apply dialog words it.
    pub message: String,
    /// The plugin to move and where, or `None` when no move can fix it.
    ///
    /// A master that is missing or switched off is not an ordering problem and
    /// has no ordering fix — offering a button that quietly did nothing would be
    /// worse than offering none.
    pub fix: Option<FixView>,
}

/// A single move that resolves one violation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixView {
    /// The plugin that moves. Always the master: moving the dependant down
    /// would satisfy this constraint by changing its relationship to every
    /// other plugin as well.
    pub name: String,
    /// The index it moves to.
    pub to: usize,
    pub label: String,
}

/// A game's plugin list, in the order the game will read it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOrderView {
    pub entries: Vec<PluginEntryView>,
    pub violations: Vec<ViolationView>,
    /// Where the list lives, for the "open folder" affordance and for anyone
    /// reporting a problem.
    pub path: String,
}

/// Where this game's plugin list is, or `None` if it does not have one.
///
/// The one place that answers this question. It was inlined in the deploy path
/// before there was a second caller; two copies of "where does this game's list
/// live" is how a screen and a deploy end up writing different files.
///
/// `None` covers three ordinary cases and no failures: a game whose engine has
/// no plugin concept, a profile that declares one without saying where it goes,
/// and a game that has never been launched under Proton and therefore has no
/// prefix yet.
pub(crate) fn plugin_list_target(state: &AppState, game_id: &str) -> Option<PluginListTarget> {
    let profile = effective_profile(state, game_id).ok()?;
    let spec = profile.plugin_list.as_ref()?;
    if !profile.manages_plugin_list() {
        return None;
    }
    let prefix = {
        let store = state.store.lock().ok()?;
        store.get_game(game_id).ok()??.proton_prefix?
    };
    PluginListTarget::resolve(Path::new(&prefix), spec)
}

/// A violation as a sentence, worded once for every surface that shows one.
pub(crate) fn violation_sentence(v: &MasterViolation) -> String {
    match v.reason {
        MasterProblem::Missing => format!(
            "{} needs {}, which is not installed or is switched off.",
            v.plugin, v.master
        ),
        MasterProblem::LoadsTooLate => format!(
            "{} loads before {}, which it depends on. Move {} above it.",
            v.plugin, v.master, v.master
        ),
    }
}

/// The directories a plugin can live in, from the profile's deploy targets.
///
/// Destinations rather than sources: this is looking for a file that has already
/// been placed. Falls back to the game directory itself, which is what an
/// engine with no declared targets means.
fn plugin_roots(state: &AppState, game_id: &str) -> Vec<String> {
    effective_profile(state, game_id)
        .map(|p| {
            p.deploy_targets
                .iter()
                .map(|t| t.target.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// The game directory, for finding the plugin files the list names.
fn game_dir_of(state: &AppState, game_id: &str) -> Option<PathBuf> {
    let store = state.store.lock().ok()?;
    store
        .get_game(game_id)
        .ok()??
        .install_dir
        .map(PathBuf::from)
}

/// Read the list as it is on disk, described from the files it names.
///
/// The list carries names and switches; a plugin's real kind and its masters
/// live in the file's own header, so the two halves are put together here.
/// `append_new` is what merges them: for a name already in the list it refreshes
/// the kind and masters from the file and leaves the position and the enabled
/// state alone, which is exactly the half each source owns.
fn read_described(state: &AppState, game_id: &str, target: &PluginListTarget) -> PluginOrder {
    let mut order = plugin_list::read(target);

    if let Some(game_dir) = game_dir_of(state, game_id) {
        let names: Vec<String> = order.entries().iter().map(|e| e.name.clone()).collect();
        let roots = plugin_roots(state, game_id);
        let rules = rules_for_state(state, game_id);
        order.append_new(apoc_modengine::plugins::entries_for_names(
            &game_dir, &names, &roots, &rules,
        ));
    }

    // Held in the order the game reads, which is the order this is about to be
    // shown in and the order it is written back in — `render` already emits
    // `as_the_game_reads_it`. Without this the three could disagree: a file
    // written by another tool can interleave masters with ordinary plugins, and
    // an index taken from the screen would then name a different entry than the
    // one the user dragged.
    //
    // Not a reordering of anybody's choices. The engine loads the master block
    // first whatever the file says, so this is the same load order written down
    // the way it actually happens.
    PluginOrder::adopt(order.as_the_game_reads_it().into_iter().cloned().collect())
}

/// Whether the named plugin is where the game would look for it.
fn is_present(state: &AppState, game_id: &str, name: &str) -> bool {
    let Some(game_dir) = game_dir_of(state, game_id) else {
        return false;
    };
    plugin_roots(state, game_id)
        .iter()
        .any(|root| game_dir.join(root).join(name).exists())
}

fn view(
    state: &AppState,
    game_id: &str,
    target: &PluginListTarget,
    order: &PluginOrder,
) -> PluginOrderView {
    // Already in the order the game reads — `read_described` normalises it, so
    // the index the screen shows, the index a move names, and the line number
    // in the file are all the same number.
    let entries = order
        .entries()
        .iter()
        .map(|e| PluginEntryView {
            name: e.name.clone(),
            enabled: e.enabled,
            kind: match e.kind {
                apoc_domain::plugins::PluginKind::Master => "master",
                apoc_domain::plugins::PluginKind::LightMaster => "light-master",
                apoc_domain::plugins::PluginKind::Normal => "normal",
            }
            .to_string(),
            masters: e.masters.clone(),
            implicit: e.implicit,
            present: e.implicit || is_present(state, game_id, &e.name),
        })
        .collect();

    let violations = order
        .violations()
        .iter()
        .map(|v| ViolationView {
            plugin: v.plugin.clone(),
            master: v.master.clone(),
            message: violation_sentence(v),
            fix: fix_for(order, v),
        })
        .collect();

    PluginOrderView {
        entries,
        violations,
        path: target.plugins_file.display().to_string(),
    }
}

/// The single move that resolves one violation, if a move can.
///
/// Only `LoadsTooLate` has one: the master is present and enabled, it is simply
/// in the wrong place, so putting it immediately above the plugin that needs it
/// is the smallest change that satisfies the constraint. A `Missing` master is
/// not installed or is switched off, which no amount of reordering fixes.
fn fix_for(order: &PluginOrder, v: &MasterViolation) -> Option<FixView> {
    if v.reason != MasterProblem::LoadsTooLate {
        return None;
    }
    let to = order.position(&v.plugin)?;
    Some(FixView {
        name: v.master.clone(),
        to,
        label: format!("Move {} above {}", v.master, v.plugin),
    })
}

/// Write the list, vaulting and journaling first.
///
/// The journal is this edit's own, and is deliberately not recorded as a
/// deployment — see the module docs.
fn commit(
    state: &AppState,
    game_id: &str,
    target: &PluginListTarget,
    order: &PluginOrder,
) -> CmdResult<()> {
    let ctx = crate::commands::build_context(state, game_id)?;

    let ops = plugin_list::write(target, &ctx.vault_dir, order).map_err(err)?;
    if ops.is_empty() {
        // Nothing changed on disk, so there is nothing to record. Creating an
        // empty journal for every click would fill the journal directory with
        // files describing that a user dragged a plugin back where it started.
        return Ok(());
    }

    let mut journal = Journal::create(
        &ctx.journal_dir,
        DeploymentHeader {
            id: apoc_deploy::journal::new_deployment_id(),
            game_id: game_id.to_string(),
            // Not a mod, so this names the edit itself. Anyone reading the
            // journal directory to work out what happened to a game folder can
            // tell this apart from a deployment at a glance.
            bundle_name: "plugin order".to_string(),
            created_at: apoc_deploy::journal::now_secs(),
            game_dir: ctx.game_dir.display().to_string(),
        },
    )
    .map_err(err)?;

    // Invariant 2. The write already happened, so this is recorded before the
    // caller is told anything about it and before the next edit can begin.
    for op in ops {
        journal.append(op).map_err(err)?;
    }
    Ok(())
}

/// The game's plugin list, or `None` for a game that does not have one.
///
/// `None` is what the interface keys its navigation off: five of the six games
/// shipping today have no plugin concept at all, and a permanently empty screen
/// is worse than no screen.
#[tauri::command(async)]
pub fn list_plugins(state: State<AppState>, game_id: String) -> CmdResult<Option<PluginOrderView>> {
    let Some(target) = plugin_list_target(&state, &game_id) else {
        return Ok(None);
    };
    let order = read_described(&state, &game_id, &target);
    Ok(Some(view(&state, &game_id, &target, &order)))
}

/// Move one plugin to a new position and write the list.
///
/// Returns the list as it now is, rather than nothing, so the screen renders
/// what is on disk instead of predicting it. The move can be refused by the
/// domain — an unknown name, or a move to where it already is — and that is not
/// an error: the answer is still the current list.
#[tauri::command(async)]
pub fn move_plugin(
    state: State<AppState>,
    game_id: String,
    name: String,
    to: usize,
) -> CmdResult<Option<PluginOrderView>> {
    let Some(target) = plugin_list_target(&state, &game_id) else {
        return Ok(None);
    };
    let mut order = read_described(&state, &game_id, &target);

    // The engine loads these itself and their position is not the user's to
    // choose. Refused here rather than in the domain, which is right to model a
    // list where somebody has already written them down in some order.
    if order.get(&name).is_some_and(|e| e.implicit) {
        return Err(format!(
            "{name} is loaded by the game itself and cannot be moved."
        ));
    }

    if order.move_to(&name, to) {
        commit(&state, &game_id, &target, &order)?;
    }
    Ok(Some(view(&state, &game_id, &target, &order)))
}

/// Switch one plugin on or off and write the list.
#[tauri::command(async)]
pub fn set_plugin_enabled(
    state: State<AppState>,
    game_id: String,
    name: String,
    enabled: bool,
) -> CmdResult<Option<PluginOrderView>> {
    let Some(target) = plugin_list_target(&state, &game_id) else {
        return Ok(None);
    };
    let mut order = read_described(&state, &game_id, &target);

    if order.get(&name).is_some_and(|e| e.implicit) {
        return Err(format!(
            "{name} is loaded by the game itself and cannot be switched off."
        ));
    }

    if order.set_enabled(&name, enabled) {
        commit(&state, &game_id, &target, &order)?;
    }
    Ok(Some(view(&state, &game_id, &target, &order)))
}
