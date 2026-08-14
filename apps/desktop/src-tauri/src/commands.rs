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
use apoc_domain::{GameProfile, ModBundle, Selection};
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
    // `profiles.game_id` is a foreign key onto `games`, so the game has to be a
    // row before a profile can point at it. Nothing guaranteed that: the row was
    // only written by detection, so on a fresh database every profile operation
    // failed with "FOREIGN KEY constraint failed" until the user happened to
    // press Find game. Detection is about where a game is installed, not about
    // whether it exists.
    ensure_game_row(&store, game_id)?;
    let id = store.ensure_profile(game_id, "Default").map_err(err)?;
    store.set_active_profile(game_id, id).map_err(err)?;
    Ok(id)
}

/// Make sure the game exists as a row, without claiming to know where it is
/// installed.
///
/// Writes only the identity — id and name, both from the bundled profile — and
/// leaves the paths null for detection to fill in. Overwriting a detected path
/// with a null here would undo the user's own Choose folder.
pub(crate) fn ensure_game_row(store: &apoc_storage::Store, game_id: &str) -> CmdResult<()> {
    if store.get_game(game_id).map_err(err)?.is_some() {
        return Ok(());
    }
    let profile = builtin_profile(game_id)?;
    store
        .upsert_game(&GameRecord {
            id: profile.id.clone(),
            name: profile.name.clone(),
            install_dir: None,
            proton_prefix: None,
            active_profile_id: None,
        })
        .map_err(err)
}

fn builtin_profile(game_id: &str) -> CmdResult<GameProfile> {
    LocalBuiltin::new().get(game_id).map_err(err)
}

/// The profile in force for a game.
///
/// Published where the service has one and the setting asks for it, bundled
/// otherwise. Reads the cache rather than the network: a profile is consulted
/// to analyse an archive and to plan a deployment, and a timeout has no place
/// in front of either.
pub(crate) fn effective_profile(state: &AppState, game_id: &str) -> CmdResult<GameProfile> {
    crate::gamedb::effective_profile(state, game_id)
        .ok_or_else(|| format!("no game profile for '{game_id}'"))
}

/// The bundled profile for a game, for callers outside this module.
pub(crate) fn game_profile(game_id: &str) -> CmdResult<GameProfile> {
    builtin_profile(game_id)
}

/// Payload-recognition rules for a game, so loader DLLs and engine-specific
/// payload roots are recognized. Falls back to defaults for an unknown game.
fn rules_for(game_id: &str) -> apoc_modengine::GameRules {
    builtin_profile(game_id)
        .map(|p| apoc_modengine::GameRules::from_profile(&p))
        .unwrap_or_default()
}

/// Payload-recognition rules under the profile actually in force.
///
/// Used wherever the state is in hand. The bundled-only [`rules_for`] remains
/// for the few callers that have no state to consult, where reading a published
/// profile is not possible and the compiled-in one is the whole truth available.
pub(crate) fn rules_for_state(state: &AppState, game_id: &str) -> apoc_modengine::GameRules {
    crate::gamedb::effective_profile(state, game_id)
        .map(|p| apoc_modengine::GameRules::from_profile(&p))
        .unwrap_or_else(|| rules_for(game_id))
}

/// Steam's own cached cover art for a game, as a data URI.
///
/// Returned inline rather than as a path because the webview cannot read
/// arbitrary files from disk, and widening the asset scope to all of Steam's
/// cache to show a thumbnail is a poor trade. The images are tens of
/// kilobytes and are fetched once per game.
///
/// `None` simply means Steam has not cached any, which is normal for a game
/// the account does not own. Nothing here reaches the network: a mod manager
/// should not be making requests to a storefront to decorate a list.
#[tauri::command(async)]
pub fn game_art(game_id: String) -> CmdResult<Option<String>> {
    use base64::Engine as _;

    let profile = builtin_profile(&game_id)?;
    let Some(path) = apoc_steam::library_art(profile.detection.steam_app_id) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(err)?;
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        _ => "image/jpeg",
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(format!("data:{mime};base64,{encoded}")))
}

/// The URL that asks Steam to run one game.
///
/// `rungameid` rather than `run`, because it is the form that also covers
/// non-Steam shortcuts and is what Steam's own library links use.
fn steam_run_url(steam_app_id: u32) -> String {
    format!("steam://rungameid/{steam_app_id}")
}

/// Ask Steam to start the game.
///
/// Deliberately the whole of it. The roadmap rules out *running* the game on
/// the grounds that a launcher wrapping Steam wrapping Proton only adds ways to
/// fail, and that still holds: nothing here manages a process, chooses a Proton
/// build, or touches launch options. It hands Steam a URL and Steam does
/// exactly what it does when the user presses Play in their own library —
/// including applying the launch options this app told them to set.
///
/// Handed to the opener from Rust rather than from the webview because the
/// window's `opener:allow-open-url` capability is scoped to two documentation
/// URLs, and widening it to a whole scheme so the UI can format a string is a
/// worse trade than keeping the app id where the profile already lives.
#[tauri::command(async)]
pub fn launch_game(app: tauri::AppHandle, game_id: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let profile = builtin_profile(&game_id)?;
    let app_id = profile.detection.steam_app_id;

    // Refused rather than handed over, because Steam's answer to an app id it
    // does not own is a store page. Offering to start a game and opening a shop
    // instead is worse than saying plainly that it is not installed.
    if apoc_steam::find_game(app_id).is_none() {
        return Err(format!(
            "{} does not look installed. Use Find game if Steam has it somewhere unusual.",
            profile.name
        ));
    }

    app.opener()
        .open_url(steam_run_url(app_id), None::<&str>)
        .map_err(err)
}

/// List all known games, merged with detection and stored configuration.
#[tauri::command(async)]
pub fn list_games(state: State<AppState>) -> CmdResult<Vec<GameView>> {
    let profiles = crate::gamedb::effective_profiles(&state);
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
        let user_reg = proton_prefix
            .as_ref()
            .map(|p| PathBuf::from(p).join("user.reg"));
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
pub fn game_for_domain(state: State<AppState>, domain: String) -> CmdResult<Option<String>> {
    let profiles = crate::gamedb::effective_profiles(&state);
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
                install_dir: detected
                    .as_ref()
                    .map(|d| d.install_dir.display().to_string()),
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

/// The selection currently in force for a given installed mod.
///
/// Takes a resolved mod id rather than finding one itself. It used to scan for a
/// matching bundle name, which meant this command and the replacement resolver
/// each had their own idea of what "the same mod" was — and two identity notions
/// in one code path is how they drift apart.
fn previous_selection(
    state: &AppState,
    game_id: &str,
    mod_id: &str,
) -> CmdResult<Option<apoc_domain::Selection>> {
    let profile_id = profile_of(state, game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(store
        .get_mod_state(profile_id, mod_id)
        .map_err(err)?
        .map(|s| s.selection))
}

/// Why an archive was taken to be a new version of an installed mod.
///
/// Ordered by how much it can be trusted. Everything above `Name` identifies the
/// mod exactly; `Name` is a guess, and the only one the user is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceReason {
    /// Byte-identical to an archive already imported: a reinstall.
    SameArchive,
    /// The download recorded which local mod it was fetched to update.
    UpdateLink,
    /// Same Nexus mod page. Two files on one page are two versions of one mod.
    NexusMod,
    /// Same bundle name, and nothing better. A guess — see [`resolve_replacement`].
    Name,
}

/// An installed mod this archive appears to be a new version of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceCandidate {
    pub mod_id: String,
    pub name: String,
    pub version: Option<String>,
    pub reason: ReplaceReason,
}

impl ReplaceCandidate {
    /// True when the match identifies the mod rather than guessing at it, and so
    /// can replace a library row without asking.
    pub fn certain(&self) -> bool {
        !matches!(self.reason, ReplaceReason::Name)
    }
}

/// Decide which installed mod, if any, an archive is a new version of.
///
/// Pure so it can be tested against a list of records rather than a database.
///
/// The name match at the bottom is deliberately the weakest and the only one
/// that is not acted on silently. A bundle name is derived, not declared: for an
/// archive carrying no metadata it falls back to the file's own stem, which for
/// a Nexus download changes with every release, and nothing makes it unique
/// within a game either. Replacing a row on that evidence would merge two
/// unrelated mods into one, so it is offered to the user instead.
///
/// An author renaming their mod is not matched by name and reads as a new mod.
/// Guessing at renames — by position, by fuzzy name — would silently replace
/// something the user did not mean, which is the worse failure.
fn resolve_replacement(
    existing: &[ModRecord],
    sha: Option<&str>,
    provenance: Option<&apoc_storage::Provenance>,
    bundle_name: &str,
) -> Option<ReplaceCandidate> {
    let candidate = |m: &ModRecord, reason: ReplaceReason| ReplaceCandidate {
        mod_id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        reason,
    };

    if let Some(sha) = sha {
        if let Some(m) = existing
            .iter()
            .find(|m| m.archive_sha256.as_deref() == Some(sha))
        {
            return Some(candidate(m, ReplaceReason::SameArchive));
        }
    }

    // A recorded update link names a row that may since have been removed. It is
    // a hint, so a stale one falls through to the weaker signals rather than
    // failing the import.
    if let Some(wanted) = provenance.and_then(|p| p.replaces_mod_id.as_deref()) {
        if let Some(m) = existing.iter().find(|m| m.id == wanted) {
            return Some(candidate(m, ReplaceReason::UpdateLink));
        }
    }

    if let Some(nexus_mod_id) = provenance.and_then(|p| p.nexus_mod_id) {
        if let Some(m) = existing
            .iter()
            .find(|m| m.nexus_mod_id == Some(nexus_mod_id))
        {
            return Some(candidate(m, ReplaceReason::NexusMod));
        }
    }

    existing
        .iter()
        .find(|m| m.name == bundle_name)
        .map(|m| candidate(m, ReplaceReason::Name))
}

/// Nanoseconds since the epoch, in hex. The trick the download queue already
/// uses to name things that need to be distinct and have nothing else to be
/// named after.
fn unique_suffix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// A library id for a mod being imported for the first time.
///
/// Derived from the archive hash where there is one, because re-importing the
/// same file should land on the same row rather than a duplicate. An archive
/// that could not be hashed gets a unique id instead of a shared placeholder:
/// every such mod used to collide on a single `mod-unknown`, so the second one
/// silently overwrote the first.
fn mint_mod_id(state: &AppState, sha: Option<&str>) -> CmdResult<String> {
    let mut id = match sha {
        Some(h) if h.len() >= 16 => format!("mod-{}", &h[..16]),
        _ => format!("mod-{}", unique_suffix()),
    };
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    // Only reachable when a hash-derived id is already taken by a mod the caller
    // did not ask to replace — a re-import that resolved to no candidate.
    while store.get_mod(&id).map_err(err)?.is_some() {
        id = format!("mod-{}", unique_suffix());
    }
    Ok(id)
}

/// The directory name for one archive's extracted files.
///
/// Distinct per import so a new version never lands on top of the one an applied
/// deployment is still repairing from. Prefixed with the mod id so a stray
/// directory can be traced back to its owner by eye.
fn new_staging_key(mod_id: &str, sha: Option<&str>) -> String {
    match sha {
        Some(h) if h.len() >= 16 => format!("{mod_id}__{}", &h[..16]),
        _ => format!("{mod_id}__{}", unique_suffix()),
    }
}

/// Where a game is installed, when it has been found. `None` is a real answer:
/// conditions that ask about the game's own files stay unknown rather than
/// being answered wrongly.
fn game_dir_of(state: &AppState, game_id: &str) -> Option<PathBuf> {
    let store = state.store.lock().ok()?;
    game_dir_in(&store, game_id)
}

/// As [`game_dir_of`], for a caller that already holds the store lock.
///
/// The mutex is not reentrant, so a command that has locked the store and then
/// calls `game_dir_of` deadlocks rather than failing.
fn game_dir_in(store: &apoc_storage::Store, game_id: &str) -> Option<PathBuf> {
    store
        .get_game(game_id)
        .ok()
        .flatten()
        .and_then(|g| g.install_dir)
        .map(PathBuf::from)
}

/// Narrow a conditional installer's view to what currently applies.
///
/// The bundle carries every option the manifest declares, because which files
/// each one installs never changes. What does change is whether an option is
/// reachable at all, so options behind a step the current answers do not reach
/// are dropped from the view rather than shown and ignored, and an option the
/// conditions forbid keeps its place but says why it cannot be chosen.
fn narrow_to_conditions(
    bundle: &ModBundle,
    game_dir: Option<&Path>,
    chosen: &Selection,
) -> CmdResult<(Vec<GroupView>, Selection, Vec<String>)> {
    let Some(module) = &bundle.fomod else {
        // Not a conditional installer. Every group applies, always.
        return Ok((groups_view(bundle), chosen.clone(), Vec::new()));
    };

    let probes = apoc_modengine::fomod::eval::probe(module, game_dir);
    let state =
        apoc_modengine::fomod::eval::evaluate(module, &probes, &chosen.chosen).map_err(err)?;

    // Everything the evaluator can currently see, and what it decided about it.
    let mut visible: std::collections::HashMap<&str, &apoc_modengine::fomod::eval::VisiblePlugin> =
        std::collections::HashMap::new();
    for step in &state.steps {
        for group in &step.groups {
            for plugin in &group.plugins {
                visible.insert(plugin.id.as_str(), plugin);
            }
        }
    }

    let mut groups = groups_view(bundle);
    for group in &mut groups {
        group.options.retain(|o| {
            // Synthetic options — required files, and the extras a combination
            // pulls in — belong to no step and are never chosen by hand.
            o.id.starts_with('@') || visible.contains_key(o.id.as_str())
        });
        for option in &mut group.options {
            let Some(plugin) = visible.get(option.id.as_str()) else {
                continue;
            };
            // A condition may forbid an option the manifest declared freely.
            if plugin.effective_type == apoc_domain::fomod::PluginTypeName::NotUsable {
                option.select_mode = "info".to_string();
                option.deployable = false;
                option.blocked_reason = Some("Your other choices rule this one out.".to_string());
            } else if plugin.locked {
                option.select_mode = "forced".to_string();
            }
        }
    }
    groups.retain(|g| !g.options.is_empty());

    let mut warnings = state.warnings.clone();
    if let Some(blocked) = &state.blocked {
        warnings.insert(
            0,
            format!("This mod says it needs {blocked}, which is not installed."),
        );
    }
    Ok((groups, state.resolved, warnings))
}

/// Re-answer a conditional installer with the choices made so far.
///
/// Pure in effect: the same archive and the same answers always give the same
/// view. No wizard session exists anywhere, so closing the window or restarting
/// mid-install loses a dialog and nothing else. The archive itself is
/// remembered between calls, because re-reading a multi-gigabyte file on every
/// click is the difference between this being usable and not.
#[tauri::command(async)]
pub fn evaluate_selection(
    state: State<AppState>,
    game_id: String,
    path: String,
    selection: Vec<String>,
) -> CmdResult<ModView> {
    let bundle = match state.cached_analysis(&path) {
        Some(cached) => cached,
        None => {
            let fresh = apoc_modengine::analyze_archive_with(
                Path::new(&path),
                &rules_for_state(&state, &game_id),
            )
            .map_err(err)?;
            state.remember_analysis(&path, &fresh);
            std::sync::Arc::new(fresh)
        }
    };

    let chosen = selection_from(&selection);
    let game_dir = game_dir_of(&state, &game_id);
    let (groups, resolved, mut warnings) =
        narrow_to_conditions(&bundle, game_dir.as_deref(), &chosen)?;
    warnings.extend(apoc_modengine::unmanaged_plugin_notice(
        &bundle,
        &rules_for_state(&state, &game_id),
    ));

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
        groups,
        selection: selection_vec(&resolved),
        total_files: bundle.deployable_options().map(|o| o.payload.len()).sum(),
        total_bytes: bundle.deployable_options().map(|o| o.total_size()).sum(),
        warnings,
    })
}

/// Analyze an archive without importing it: powers the wizard preview.
#[tauri::command(async)]
pub fn analyze_archive(
    state: State<AppState>,
    game_id: String,
    path: String,
) -> CmdResult<AnalyzedArchive> {
    let bundle =
        apoc_modengine::analyze_archive_with(Path::new(&path), &rules_for_state(&state, &game_id))
            .map_err(err)?;
    // Remembered so the wizard's first re-evaluation does not read the archive
    // again. Analysing is the expensive half; evaluating is arithmetic.
    state.remember_analysis(&path, &bundle);

    let replaces = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        let existing = store.list_mods(&game_id).map_err(err)?;
        let provenance = store.archive_provenance(&path).map_err(err)?;
        resolve_replacement(
            &existing,
            bundle.archive_sha256.as_deref(),
            provenance.as_ref(),
            &bundle.name,
        )
    };

    // If this archive is a newer release of something already installed, carry
    // the choices that were made last time rather than starting from a blank
    // slate. Rebuilding a body variant and three addons from memory on every
    // update is the difference between updating being a click and a chore.
    //
    // The caller gets the whole outcome, not just the selection: whether the
    // wizard needs to open at all is its decision, and it cannot make it
    // without knowing what failed to carry.
    let previous = match &replaces {
        Some(c) => previous_selection(&state, &game_id, &c.mod_id)?,
        None => None,
    };
    let (selection, carry) = match previous {
        Some(previous) => {
            let carried = apoc_modengine::carry_selection(&bundle, &previous);
            let view = carry_view(&bundle, &carried);
            (carried.selection, Some(view))
        }
        None => (apoc_modengine::default_selection(&bundle), None),
    };

    // A conditional installer opens on the steps its starting answers reach,
    // not on every step it declares. Showing gated ones and only hiding them
    // after the first click would offer choices the author meant to withhold.
    let game_dir = game_dir_of(&state, &game_id);
    let (groups, selection, mut warnings) =
        narrow_to_conditions(&bundle, game_dir.as_deref(), &selection)?;
    // Not a FOMOD concern: any format can ship a plugin, and the game will
    // ignore all of them until somebody enables them.
    warnings.extend(apoc_modengine::unmanaged_plugin_notice(
        &bundle,
        &rules_for_state(&state, &game_id),
    ));

    let mod_view = ModView {
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
        groups,
        selection: selection_vec(&selection),
        total_files: bundle.deployable_options().map(|o| o.payload.len()).sum(),
        total_bytes: bundle.deployable_options().map(|o| o.total_size()).sum(),
        warnings,
    };
    Ok(AnalyzedArchive {
        mod_view,
        carry,
        replaces: replaces.map(|c| ReplaceCandidateView {
            certain: c.certain(),
            mod_id: c.mod_id,
            name: c.name,
            version: c.version,
        }),
    })
}

/// Describe a carry outcome in the terms the person who made the choices used.
///
/// `carried` deliberately lists only options someone actually picked, not the
/// forced entries `default_selection` adds back — telling a user their mod
/// "kept" a base-files entry they never chose reads as noise, and hides the two
/// real choices in a list of nine.
fn carry_view(
    bundle: &apoc_domain::ModBundle,
    carried: &apoc_modengine::CarriedSelection,
) -> CarryView {
    let forced = apoc_modengine::default_selection(bundle);
    let kept = bundle
        .options()
        .filter(|o| carried.selection.contains(&o.id) && !forced.contains(&o.id))
        .map(|o| o.name.clone())
        .collect();

    // A dropped id may still name an option in the new bundle — one demoted to a
    // notice or a header — in which case its name is more use than its id.
    let dropped = carried
        .dropped
        .iter()
        .map(|id| match bundle.options().find(|o| &o.id == id) {
            Some(o) => o.name.clone(),
            None => id.clone(),
        })
        .collect();

    CarryView {
        carried: kept,
        dropped,
        undecided: carried.undecided.clone(),
        complete: carried.is_complete(),
    }
}

/// Import a mod: analyze, stage its payloads, and register it in the active profile.
#[tauri::command(async)]
pub fn import_mod(
    state: State<AppState>,
    game_id: String,
    archive_path: String,
    selection: Vec<String>,
    replaces: Option<String>,
) -> CmdResult<ModView> {
    let path = PathBuf::from(&archive_path);
    let bundle = apoc_modengine::analyze_archive_with(&path, &rules_for_state(&state, &game_id))
        .map_err(err)?;

    let provenance = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.archive_provenance(&archive_path).map_err(err)?
    };

    // Replacing keeps the mod's identity and everything keyed to it: its place
    // in load order, whether it is enabled, and any conflict override naming it.
    // A fresh id would need all three copied across by hand, and `delete_mod`
    // cascades them away in every profile, not just this one.
    let (mod_id, previous_state) = match &replaces {
        Some(id) => {
            let store = state.store.lock().map_err(|_| "state poisoned")?;
            // A caller asking to replace a row that is gone must not quietly get
            // a new mod instead — that is exactly the duplicate this exists to
            // prevent, and it would be invisible.
            let existing = store.get_mod(id).map_err(err)?.ok_or_else(|| {
                "The mod this update replaces is no longer installed.".to_string()
            })?;
            if existing.game_id != game_id {
                return Err("That mod belongs to a different game.".to_string());
            }
            let profile_id = profile_of(&state, &game_id)?;
            let st = store.get_mod_state(profile_id, id).map_err(err)?;
            (existing.id, st)
        }
        None => (mint_mod_id(&state, bundle.archive_sha256.as_deref())?, None),
    };

    // A generation of its own, always. Staging over the previous one would
    // destroy the bytes an applied deployment repairs from, and `stage_bundle`
    // merges into its destination rather than clearing it, so the damage would
    // be partial and silent.
    let staging_key = new_staging_key(&mod_id, bundle.archive_sha256.as_deref());

    state.paths.ensure_game_dirs(&game_id).map_err(err)?;
    let staging = staging_for(&state.paths, &game_id, &staging_key);
    apoc_modengine::stage_bundle(&path, &bundle, &staging).map_err(err)?;

    let sel = if selection.is_empty() {
        apoc_modengine::default_selection(&bundle)
    } else {
        selection_from(&selection)
    };

    // An update inherits how the mod was already set up. Writing the defaults
    // would re-enable a mod the user had turned off and drop it to the bottom of
    // load order, both of which read as the app undoing their decisions.
    let enabled = previous_state.as_ref().map(|s| s.enabled).unwrap_or(true);
    let priority = previous_state.as_ref().map(|s| s.priority).unwrap_or(0);

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
                // Carried from what the download recorded. A mod added from disk
                // has no Nexus identity, so an update check has nothing to ask
                // about, which is correct.
                nexus_mod_id: provenance.as_ref().and_then(|p| p.nexus_mod_id),
                nexus_file_id: provenance.as_ref().and_then(|p| p.nexus_file_id),
                staging_key: staging_key.clone(),
                bundle: bundle.clone(),
            })
            .map_err(err)?;
        store
            .set_mod_state(
                profile_id,
                &ModState {
                    mod_id: mod_id.clone(),
                    enabled,
                    priority,
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
        enabled,
        priority,
        applied: false,
        added_at: 0,
        groups: groups_view(&bundle),
        selection: selection_vec(&sel),
        total_files: bundle.deployable_options().map(|o| o.payload.len()).sum(),
        total_bytes: bundle.deployable_options().map(|o| o.total_size()).sum(),
        warnings: Vec::new(),
    })
}

/// List installed mods for a game, with their state in the active profile.
#[tauri::command(async)]
pub fn list_mods(state: State<AppState>, game_id: String) -> CmdResult<Vec<ModView>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    let mods = store.list_mods(&game_id).map_err(err)?;
    let applied = store.applied_mod_ids(&game_id).map_err(err)?;
    // Read once, from the lock this function already holds: `game_dir_of` takes
    // it again and the mutex is not reentrant.
    let game_dir = game_dir_in(&store, &game_id);

    let mut out = Vec::new();
    for m in mods {
        let st = store.get_mod_state(profile_id, &m.id).map_err(err)?;
        let sel = st
            .as_ref()
            .map(|s| s.selection.clone())
            .unwrap_or_else(|| apoc_modengine::default_selection(&m.bundle));
        // Narrowed with the stored answers, exactly as the wizard narrows with
        // the pending ones. Editing an installed FOMOD showed every option the
        // manifest declares, flat -- including ones behind steps those answers
        // never reach -- and ticking one installed it.
        let (groups, _resolved, _warnings) =
            narrow_to_conditions(&m.bundle, game_dir.as_deref(), &sel)?;
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
            groups,
            selection: selection_vec(&sel),
            total_files: m.bundle.deployable_options().map(|o| o.payload.len()).sum(),
            total_bytes: m.bundle.deployable_options().map(|o| o.total_size()).sum(),
            warnings: Vec::new(),
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

/// Enable or disable many mods at once, all or nothing.
///
/// Not a loop over [`set_mod_enabled`] on the front end, for two reasons. The
/// store writes the batch in one transaction, so a failure partway leaves the
/// profile as it was rather than in a state nobody selected. And this is
/// `async`, unlike its single-mod sibling: forty sequential commands would take
/// the store mutex forty times on the main thread, which is the one thing a
/// bulk action exists to avoid.
#[tauri::command(async)]
pub fn set_mods_enabled(
    state: State<AppState>,
    game_id: String,
    mod_ids: Vec<String>,
    enabled: bool,
) -> CmdResult<usize> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .set_enabled_bulk(profile_id, &mod_ids, enabled)
        .map_err(err)
}

/// What a requested selection actually becomes once the conditions and the
/// group cardinalities have had their say.
///
/// Kept separate from the command so it can be tested without a store, and so
/// the rule it encodes is stated once: **what the narrowed view permits is what
/// may be stored.** The dialog filters too, but a view is never the
/// authorization boundary -- a request can arrive with any ids in it.
///
/// Both halves read from `narrowed`, not from the manifest. An option behind a
/// step the answers do not reach is absent from it, and one the conditions
/// forbid comes back as `info`, which is the same answer the manifest already
/// gives for an option that was never selectable.
fn settled_selection(
    bundle: &ModBundle,
    narrowed: &[GroupView],
    requested: &[String],
) -> Selection {
    let permitted: std::collections::HashSet<&str> = narrowed
        .iter()
        .flat_map(|g| g.options.iter())
        .filter(|o| o.select_mode != "info")
        .map(|o| o.id.as_str())
        .collect();

    // Re-apply exclusivity so the stored selection is always internally valid.
    let mut sel = Selection::new();
    for id in requested {
        if !permitted.contains(id.as_str()) {
            continue;
        }
        let Some(opt) = bundle.options().find(|o| &o.id == id) else {
            continue;
        };
        match opt.select_mode {
            apoc_domain::SelectMode::Info => {}
            apoc_domain::SelectMode::Exclusive => {
                apoc_modengine::choose_exclusive(bundle, &mut sel, id)
            }
            _ => sel.insert(id.clone()),
        }
    }
    // A step the answers do not reach can still declare a forced option, and
    // forcing it would install files from a branch the user never took.
    for o in narrowed.iter().flat_map(|g| g.options.iter()) {
        if o.select_mode == "forced" && o.deployable {
            sel.insert(o.id.clone());
        }
    }
    sel
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

    // Narrowed against the incoming selection rather than the stored one,
    // because the incoming answers are what decide which steps this selection
    // reaches.
    let incoming = selection_from(&selection);
    let game_dir = game_dir_in(&store, &game_id);
    let (narrowed, _resolved, _warnings) =
        narrow_to_conditions(&m.bundle, game_dir.as_deref(), &incoming)?;
    let sel = settled_selection(&m.bundle, &narrowed, &selection);

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
    // From the profile in force, not the bundled one. A patch chain or a
    // copy-only path that changed in a game update is exactly the kind of
    // correction publishing profiles exists to deliver, and a deployment
    // reading the stale one would put a mod in a slot the game no longer reads.
    let profile = effective_profile(state, game_id).ok();
    Ok(DeployContext {
        game_id: game_id.to_string(),
        game_dir: PathBuf::from(install_dir),
        staging_dir: state.paths.staging_root(game_id),
        vault_dir: state.paths.vault(game_id),
        journal_dir: state.paths.journal(game_id),
        ladder: Ladder::default(),
        pak_chain: profile.as_ref().and_then(|p| p.pak_chain.clone()),
        copy_only_paths: profile
            .as_ref()
            .map(DeployContext::copy_only_from)
            .unwrap_or_default(),
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
            staging_key: m.staging_key.clone(),
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
pub(crate) fn revert_current(
    state: &AppState,
    game_id: &str,
    ctx: &DeployContext,
) -> CmdResult<()> {
    let outstanding = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store.applied_deployments(game_id).map_err(err)?
    };

    // Whether the game directory is genuinely back to how it was. Pruning below
    // depends on it: a rollback that failed has left files in the game, and
    // deleting the staging they came from would take away the only source
    // `repair` could put them back from.
    let mut all_reverted = true;

    for (dep_id, journal_path) in outstanding {
        match apoc_deploy::journal::Journal::load(Path::new(&journal_path)) {
            // The loader override is left in place; only files are reconciled here.
            // `is_clean` is the right test rather than "no errors": a file left
            // in place because its bytes changed under us is still in the game
            // folder, and its staging is still the only thing repair could use.
            Ok(journal) => {
                if !apoc_deploy::rollback(ctx, &journal, None).is_clean() {
                    all_reverted = false;
                }
            }
            Err(_) => all_reverted = false,
        }
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .record_deployment(&dep_id, game_id, None, &journal_path, "reverted")
            .map_err(err)?;
    }

    // Nothing is applied now, so every staging directory the library still names
    // is live and everything else is a superseded generation left behind by an
    // update. This is the only moment that is true, which is why pruning happens
    // here rather than at import.
    if all_reverted {
        let keep: std::collections::HashSet<String> = {
            let store = state.store.lock().map_err(|_| "state poisoned")?;
            store
                .staging_keys(game_id)
                .map_err(err)?
                .into_iter()
                .collect()
        };
        // Reclaiming disk is a courtesy, not part of reverting. Failing it must
        // not fail an apply the user asked for.
        let _ = prune_staging(&state.paths.staging_root(game_id), &keep);
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
    let staging_key = {
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
        // Read the key before the row goes: after `delete_mod` there is nothing
        // left to say which directory belonged to this mod.
        store.get_mod(&mod_id).map_err(err)?.map(|m| m.staging_key)
    };

    // Delete the staged payload before the record, so a failure here leaves the
    // mod visible and removable rather than orphaning files with no owner.
    let Some(staging_key) = staging_key else {
        // No row, nothing staged to own. Deleting again is not an error.
        return Ok(());
    };
    let staging = staging_for(&state.paths, &game_id, &staging_key);
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
            g.install_dir
                .ok_or_else(|| "install dir unknown".to_string())?,
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
        copy_only_paths: builtin_profile(&game_id)
            .ok()
            .map(|p| DeployContext::copy_only_from(&p))
            .unwrap_or_default(),
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
        return Err(if cfg!(windows) {
            "Close Steam first.".into()
        } else {
            "Close Steam before changing the Proton prefix.".to_string()
        });
    }
    // The profile in force. A loader whose override string changed with a game
    // update is one of the things publishing profiles is for, and setting up
    // the prefix from a stale definition writes the wrong registry key.
    let profile = effective_profile(&state, &game_id)?;
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

    // Windows needs none of this, and demanding it made the loader impossible
    // to set up there.
    //
    // A DLL override is a Wine concept: it exists to tell Wine to prefer a
    // native DLL over its own builtin. Windows has no builtin to prefer over,
    // so a proxy DLL sitting in the game folder is loaded because that is
    // simply how the loader search order works. The copy happens during apply;
    // there is nothing left for this command to register.
    //
    // It reports success rather than refusing, because from where the user is
    // standing the loader *is* set up — refusing would send them looking for a
    // Proton prefix that cannot exist on their machine.
    #[cfg(windows)]
    {
        let _ = overrides;
        return Ok("No registration is needed on Windows: the loader DLL is placed in the game folder when you apply.".to_string());
    }

    #[cfg(not(windows))]
    let prefix = {
        let store = state.store.lock().map_err(|_| "state poisoned")?;
        store
            .get_game(&game_id)
            .map_err(err)?
            .and_then(|g| g.proton_prefix)
            .ok_or_else(|| {
                "Proton prefix not found: run the game once via Steam first.".to_string()
            })?
    };
    #[cfg(not(windows))]
    {
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

/// Which profiles are actually being used, and when they last arrived.
///
/// Worth asking separately from the setting, because the two can disagree: an
/// app that selected the published profiles and then could not reach the
/// service looks exactly like one that is working.
#[tauri::command]
pub fn game_db_status(state: State<AppState>) -> CmdResult<crate::gamedb::ProfileSourceView> {
    Ok(crate::gamedb::source_view(&state))
}

/// Fetch the published game profiles now.
///
/// Only ever called by somebody pressing a button, which is why fetching lives
/// here rather than on the path of every profile read: analysing an archive and
/// planning a deployment both consult a profile, and neither should wait on a
/// network timeout to do it.
#[tauri::command(async)]
pub fn refresh_game_db(state: State<AppState>) -> CmdResult<String> {
    crate::gamedb::refresh(&state, env!("CARGO_PKG_VERSION"))
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
        // Same foreign key as profile_of: naming a new profile for a game that
        // has never been detected must not fail on the constraint.
        ensure_game_row(&store, &game_id)?;
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
        store
            .set_active_profile(&game_id, profile_id)
            .map_err(err)?;
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
    let mime = match filename
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
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
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Preview image for one option of a not-yet-imported archive (import wizard).
#[tauri::command(async)]
pub fn preview_from_archive(
    state: State<AppState>,
    game_id: String,
    archive_path: String,
    option_id: String,
) -> CmdResult<Option<String>> {
    let path = PathBuf::from(&archive_path);
    let bundle =
        apoc_modengine::analyze_archive_no_hash_with(&path, &rules_for_state(&state, &game_id))
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

    let staging = staging_for(&state.paths, &game_id, &m.staging_key);
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

/// What may be stored, once the conditions have been applied.
///
/// These drive [`settled_selection`] with a narrowed view built by hand, which
/// is the boundary the fix lives on: `narrow_to_conditions` decides what the
/// view contains, and this decides what a request may do with it.
#[cfg(test)]
mod settled_selection_tests {
    use super::*;
    use apoc_domain::{InstallerModel, ModOption, OptionGroup, SelectMode};

    fn opt(id: &str, mode: SelectMode, radio_set: Option<&str>) -> ModOption {
        ModOption {
            id: id.to_string(),
            folder_name: id.to_string(),
            group_index: None,
            slot_token: None,
            radio_set: radio_set.map(str::to_string),
            name: id.to_string(),
            description: None,
            category: None,
            author: None,
            screenshot: None,
            screenshot_archive_path: None,
            select_mode: mode,
            recommended: false,
            blocked_reason: None,
            deployable: mode != SelectMode::Info,
            payload: Vec::new(),
            raw_modinfo: Default::default(),
        }
    }

    fn bundle(options: Vec<ModOption>) -> ModBundle {
        ModBundle {
            name: "Test".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::Fomod,
            archive_sha256: None,
            fomod: None,
            groups: vec![OptionGroup {
                index: None,
                label: "Group".into(),
                cardinality: None,
                options,
            }],
        }
    }

    /// One narrowed group holding `(id, select_mode)` pairs.
    fn view(options: &[(&str, &str)]) -> Vec<GroupView> {
        vec![GroupView {
            index: None,
            label: "Group".into(),
            radio_sets: Vec::new(),
            cardinality: None,
            options: options
                .iter()
                .map(|(id, mode)| OptionView {
                    id: (*id).to_string(),
                    name: (*id).to_string(),
                    description: None,
                    select_mode: (*mode).to_string(),
                    radio_set: None,
                    deployable: *mode != "info",
                    file_count: 0,
                    size_bytes: 0,
                    screenshot: None,
                    has_preview: false,
                    category: None,
                    recommended: false,
                    blocked_reason: None,
                })
                .collect(),
        }]
    }

    fn ids(sel: &Selection) -> Vec<String> {
        let mut out = selection_vec(sel);
        out.sort();
        out
    }

    #[test]
    fn an_option_the_narrowed_view_does_not_contain_cannot_be_stored() {
        // The step it lives behind is not reached, so the wizard never showed
        // it. The request can still name it, and this is what refuses.
        let b = bundle(vec![
            opt("reachable", SelectMode::Stackable, None),
            opt("behind-a-step-not-taken", SelectMode::Stackable, None),
        ]);
        let narrowed = view(&[("reachable", "stackable")]);

        let sel = settled_selection(
            &b,
            &narrowed,
            &[
                "reachable".to_string(),
                "behind-a-step-not-taken".to_string(),
            ],
        );
        assert_eq!(ids(&sel), ["reachable"]);
    }

    #[test]
    fn an_option_the_conditions_rule_out_cannot_be_stored() {
        // Present in the view, but narrowed to `info` because another answer
        // forbids it.
        let b = bundle(vec![
            opt("kept", SelectMode::Stackable, None),
            opt("ruled-out", SelectMode::Stackable, None),
        ]);
        let narrowed = view(&[("kept", "stackable"), ("ruled-out", "info")]);

        let sel = settled_selection(
            &b,
            &narrowed,
            &["kept".to_string(), "ruled-out".to_string()],
        );
        assert_eq!(ids(&sel), ["kept"]);
    }

    #[test]
    fn a_forced_option_out_of_reach_is_not_forced_in() {
        // The manifest forces it; the branch it sits on was never taken. Adding
        // it would install files from a step the user did not visit.
        let b = bundle(vec![
            opt("here", SelectMode::Stackable, None),
            opt("forced-elsewhere", SelectMode::Forced, None),
        ]);
        let narrowed = view(&[("here", "stackable")]);

        let sel = settled_selection(&b, &narrowed, &["here".to_string()]);
        assert_eq!(ids(&sel), ["here"]);
    }

    #[test]
    fn a_forced_option_in_reach_is_added_without_being_asked_for() {
        let b = bundle(vec![
            opt("here", SelectMode::Stackable, None),
            opt("core", SelectMode::Forced, None),
        ]);
        let narrowed = view(&[("here", "stackable"), ("core", "forced")]);

        let sel = settled_selection(&b, &narrowed, &["here".to_string()]);
        assert_eq!(ids(&sel), ["core", "here"]);
    }

    #[test]
    fn exclusivity_still_applies_to_what_survives() {
        // The narrowing is a filter in front of the existing rules, not a
        // replacement for them.
        let b = bundle(vec![
            opt("slim", SelectMode::Exclusive, Some("shape")),
            opt("curvy", SelectMode::Exclusive, Some("shape")),
        ]);
        let narrowed = view(&[("slim", "exclusive"), ("curvy", "exclusive")]);

        let sel = settled_selection(&b, &narrowed, &["slim".to_string(), "curvy".to_string()]);
        assert_eq!(ids(&sel), ["curvy"], "the last one named wins the set");
    }

    #[test]
    fn a_bundle_that_is_not_a_conditional_installer_is_unaffected() {
        // `narrow_to_conditions` hands back every group for these, so the view
        // permits everything and the old behaviour stands.
        let b = bundle(vec![
            opt("a", SelectMode::Stackable, None),
            opt("b", SelectMode::Stackable, None),
        ]);
        let narrowed = narrow_to_conditions(&b, None, &Selection::new()).unwrap().0;

        let sel = settled_selection(&b, &narrowed, &["a".to_string(), "b".to_string()]);
        assert_eq!(ids(&sel), ["a", "b"]);
    }
}

#[cfg(test)]
mod carry_view_tests {
    use super::*;
    use apoc_domain::{InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode, Selection};

    fn opt(id: &str, name: &str, mode: SelectMode) -> ModOption {
        ModOption {
            id: id.to_string(),
            folder_name: id.to_string(),
            group_index: None,
            slot_token: None,
            radio_set: None,
            name: name.to_string(),
            description: None,
            category: None,
            author: None,
            screenshot: None,
            screenshot_archive_path: None,
            select_mode: mode,
            recommended: false,
            blocked_reason: None,
            deployable: mode != SelectMode::Info,
            payload: Vec::new(),
            raw_modinfo: Default::default(),
        }
    }

    fn bundle(options: Vec<ModOption>) -> ModBundle {
        ModBundle {
            name: "Test".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::FluffyAio,
            archive_sha256: None,
            fomod: None,
            groups: vec![OptionGroup {
                index: None,
                label: "Group".into(),
                cardinality: None,
                options,
            }],
        }
    }

    fn selection(ids: &[&str]) -> Selection {
        let mut s = Selection::new();
        for i in ids {
            s.insert((*i).to_string());
        }
        s
    }

    #[test]
    fn carried_names_the_choices_someone_made_not_the_forced_ones() {
        let b = bundle(vec![
            opt("core", "Base files", SelectMode::Forced),
            opt("addon-a", "Glowing eyes", SelectMode::Stackable),
            opt("addon-b", "Extra straps", SelectMode::Stackable),
        ]);
        let carried = apoc_modengine::carry_selection(&b, &selection(&["core", "addon-a"]));
        let view = carry_view(&b, &carried);

        assert_eq!(view.carried, vec!["Glowing eyes"]);
        assert!(
            !view.carried.contains(&"Base files".to_string()),
            "forced entries are not choices anyone made"
        );
        assert!(view.complete);
    }

    #[test]
    fn a_dropped_option_that_is_gone_is_reported_by_its_folder_name() {
        // Nothing in the new bundle can name it, so the id it had is all there
        // is to say — and it is what the person will recognise on disk.
        let b = bundle(vec![opt("core", "Base files", SelectMode::Forced)]);
        let carried = apoc_modengine::carry_selection(&b, &selection(&["core", "addon-gone"]));
        let view = carry_view(&b, &carried);

        assert_eq!(view.dropped, vec!["addon-gone"]);
        assert!(!view.complete);
    }

    #[test]
    fn a_dropped_option_still_present_is_reported_by_its_name() {
        // Demoted to a notice: it exists, so it has a name worth showing.
        let b = bundle(vec![
            opt("core", "Base files", SelectMode::Forced),
            opt("addon-a", "Glowing eyes", SelectMode::Info),
        ]);
        let carried = apoc_modengine::carry_selection(&b, &selection(&["core", "addon-a"]));
        let view = carry_view(&b, &carried);

        assert_eq!(view.dropped, vec!["Glowing eyes"]);
        assert!(!view.complete);
    }

    #[test]
    fn a_new_choice_set_leaves_the_install_incomplete() {
        let mut variant = opt("body-a", "Body A", SelectMode::Exclusive);
        variant.radio_set = Some("body".into());
        let b = bundle(vec![opt("core", "Base files", SelectMode::Forced), variant]);
        let carried = apoc_modengine::carry_selection(&b, &selection(&["core"]));
        let view = carry_view(&b, &carried);

        assert_eq!(view.undecided, vec!["body"]);
        assert!(
            !view.complete,
            "a question with no answer must open the wizard"
        );
    }
}

#[cfg(test)]
mod resolve_replacement_tests {
    use super::*;
    use apoc_domain::{InstallerModel, ModBundle};
    use apoc_storage::Provenance;

    fn record(id: &str, name: &str) -> ModRecord {
        ModRecord {
            id: id.into(),
            game_id: "monster-hunter-wilds".into(),
            name: name.into(),
            version: None,
            author: None,
            archive_path: format!("/downloads/{id}.zip"),
            archive_sha256: None,
            installer_model: "fluffy-aio".into(),
            imported_at: 0,
            nexus_mod_id: None,
            nexus_file_id: None,
            staging_key: id.into(),
            bundle: ModBundle {
                name: name.into(),
                version: None,
                author: None,
                category: None,
                installer_model: InstallerModel::FluffyAio,
                archive_sha256: None,
                fomod: None,
                groups: Vec::new(),
            },
        }
    }

    fn provenance(replaces: Option<&str>, nexus_mod_id: Option<i64>) -> Provenance {
        Provenance {
            archive_path: "/downloads/new.zip".into(),
            domain: Some("monsterhunterwilds".into()),
            nexus_mod_id,
            nexus_file_id: Some(99),
            replaces_mod_id: replaces.map(str::to_string),
        }
    }

    #[test]
    fn nothing_installed_means_nothing_to_replace() {
        assert!(resolve_replacement(&[], Some("abc"), None, "Anything").is_none());
    }

    #[test]
    fn the_same_archive_is_the_same_mod() {
        let mut existing = record("mod-a", "Armour Overhaul");
        existing.archive_sha256 = Some("deadbeef".into());
        let found =
            resolve_replacement(&[existing], Some("deadbeef"), None, "Renamed Since").unwrap();

        assert_eq!(found.mod_id, "mod-a");
        assert_eq!(found.reason, ReplaceReason::SameArchive);
        assert!(found.certain(), "byte-identical is not a guess");
    }

    #[test]
    fn a_recorded_update_link_beats_a_name_that_matches_something_else() {
        // The download said what it was for. Nothing weaker should override it.
        let existing = vec![record("mod-a", "Armour Overhaul"), record("mod-b", "Other")];
        let found = resolve_replacement(
            &existing,
            None,
            Some(&provenance(Some("mod-b"), None)),
            "Armour Overhaul",
        )
        .unwrap();

        assert_eq!(found.mod_id, "mod-b");
        assert_eq!(found.reason, ReplaceReason::UpdateLink);
        assert!(found.certain());
    }

    #[test]
    fn an_update_link_naming_a_removed_mod_falls_through_rather_than_failing() {
        // The row it named is gone. That is not a reason to refuse the import,
        // only a reason to stop trusting the hint.
        let existing = vec![record("mod-a", "Armour Overhaul")];
        let found = resolve_replacement(
            &existing,
            None,
            Some(&provenance(Some("mod-deleted"), None)),
            "Armour Overhaul",
        )
        .unwrap();

        assert_eq!(found.mod_id, "mod-a");
        assert_eq!(found.reason, ReplaceReason::Name, "fell back to the guess");
    }

    #[test]
    fn the_same_nexus_page_is_the_same_mod_whatever_it_is_called() {
        // The strongest signal for a file fetched by nxm://, which never says
        // which local mod it is for. A release renamed on the page still matches.
        let mut existing = record("mod-a", "Armour Overhaul");
        existing.nexus_mod_id = Some(1234);
        let found = resolve_replacement(
            &[existing],
            None,
            Some(&provenance(None, Some(1234))),
            "Armour Overhaul Redux",
        )
        .unwrap();

        assert_eq!(found.mod_id, "mod-a");
        assert_eq!(found.reason, ReplaceReason::NexusMod);
        assert!(found.certain());
    }

    #[test]
    fn a_bare_name_match_is_offered_rather_than_acted_on() {
        let found = resolve_replacement(
            &[record("mod-a", "Armour Overhaul")],
            None,
            None,
            "Armour Overhaul",
        )
        .unwrap();

        assert_eq!(found.mod_id, "mod-a");
        assert_eq!(found.reason, ReplaceReason::Name);
        assert!(
            !found.certain(),
            "a shared name could be two different mods, so this must be asked about"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launch_url_is_the_one_steam_answers_to() {
        // The whole of launching is this string, so it is the whole of what
        // there is to get wrong. `rungameid` is the form Steam's own library
        // links use; `run` exists but does not cover non-Steam shortcuts.
        assert_eq!(steam_run_url(1091500), "steam://rungameid/1091500");
        assert_eq!(steam_run_url(2246340), "steam://rungameid/2246340");
    }

    #[test]
    fn both_shipped_games_produce_a_launch_url() {
        // Reads the app id from the profile rather than restating it, so a
        // profile that loses its detection block fails here.
        for id in ["monster-hunter-wilds", "cyberpunk-2077"] {
            let p = builtin_profile(id).expect("profile ships");
            let url = steam_run_url(p.detection.steam_app_id);
            assert!(url.starts_with("steam://rungameid/"), "{id}: {url}");
            assert!(
                url.rsplit('/')
                    .next()
                    .is_some_and(|n| n.parse::<u32>().is_ok()),
                "{id}: {url} does not end in an app id"
            );
        }
    }
}
