//! Named blocks of mods in the load order, and the drag that moves things.
//!
//! Two things live here, and they are here together because they are the same
//! subject: a group is a promise about where mods sit, and a drag is the thing
//! that would break it.
//!
//! **The drag is sent, not its result.** Every other reorder command in this
//! application takes the finished arrangement, which works because the list that
//! produced it showed everything. This list does not: it can be searched,
//! filtered, sorted and collapsed, so the row somebody dropped below is very
//! often not the entry above it in the true order, and a client that has not
//! seen the last change would name a position that has since moved. So the
//! screen sends what the person did, anchored to the row they did it against,
//! and the store replays it against the order it currently holds.
//!
//! **Refusals come from the store, not from here.** A locked group is enforced
//! in `apoc-storage`, which is the one door the command line and every future
//! automatic sort also pass through. These commands surface the sentence; they
//! are not the reason it is true.

use crate::commands::{err, profile_of};
use crate::state::{AppState, ModGroupView};
use apoc_domain::modgroups::OrderMove;
use tauri::State;

type CmdResult<T> = Result<T, String>;

fn view_of(record: apoc_storage::ModGroupRecord) -> ModGroupView {
    ModGroupView {
        id: record.group.id,
        name: record.group.name,
        color: record.group.color,
        locked: record.group.locked,
        collapsed: record.group.collapsed,
    }
}

/// Every group in the game's active profile, in the order their blocks appear.
#[tauri::command]
pub fn list_mod_groups(state: State<AppState>, game_id: String) -> CmdResult<Vec<ModGroupView>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    Ok(store
        .list_groups(profile_id)
        .map_err(err)?
        .into_iter()
        .map(view_of)
        .collect())
}

/// Make a group. It holds nothing until something is put in it.
#[tauri::command]
pub fn create_mod_group(
    state: State<AppState>,
    game_id: String,
    name: String,
    color: String,
) -> CmdResult<i64> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.create_group(profile_id, &name, &color).map_err(err)
}

/// Rename, recolour or collapse a group. All three are allowed while locked: a
/// lock holds the order, and refusing a typo fix would teach people to unlock
/// out of habit.
#[tauri::command]
pub fn update_mod_group(
    state: State<AppState>,
    group_id: i64,
    name: Option<String>,
    color: Option<String>,
    collapsed: Option<bool>,
) -> CmdResult<()> {
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .update_group(group_id, name.as_deref(), color.as_deref(), collapsed)
        .map_err(err)
}

/// Lock or unlock a group. Locking gathers its mods together first, so the
/// guarantee it makes is one that actually holds when it is made.
#[tauri::command]
pub fn set_mod_group_locked(
    state: State<AppState>,
    game_id: String,
    group_id: i64,
    locked: bool,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .set_group_locked(profile_id, group_id, locked)
        .map_err(err)
}

/// Delete a group. Its mods stay exactly where they are in the order.
#[tauri::command]
pub fn delete_mod_group(state: State<AppState>, game_id: String, group_id: i64) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.delete_group(profile_id, group_id).map_err(err)
}

/// Put mods in a group, or take them out of one with a null group.
#[tauri::command]
pub fn assign_to_mod_group(
    state: State<AppState>,
    game_id: String,
    group_id: Option<i64>,
    mod_ids: Vec<String>,
) -> CmdResult<()> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store
        .assign_to_group(profile_id, group_id, &mod_ids)
        .map_err(err)
}

/// Replay one drag and return the order it produced.
#[tauri::command]
pub fn move_in_order(
    state: State<AppState>,
    game_id: String,
    r#move: OrderMove,
) -> CmdResult<Vec<String>> {
    let profile_id = profile_of(&state, &game_id)?;
    let store = state.store.lock().map_err(|_| "state poisoned")?;
    store.move_in_order(profile_id, &r#move).map_err(err)
}
