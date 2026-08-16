//! Which game profiles are in force.
//!
//! Two sources exist: the profiles compiled into this build, and the ones the
//! service publishes. The setting chooses between them, and until now it chose
//! nothing at all — every profile read in the app went straight to the bundled
//! set regardless of what it said.
//!
//! The rule that decides everything here: **the bundled profiles are the floor.**
//! Selecting the online source can add games and correct the ones that shipped,
//! and it can never take modding away. Offline, mid-outage, signed out, or
//! reading a document written for a contract this build does not know, the
//! answer falls back to what is compiled in — because none of those is a
//! condition anyone agreed to when they installed a mod manager.
//!
//! Fetching happens when asked, not on the path of every read. A profile is
//! consulted to analyse an archive, to plan a deployment, to draw the library;
//! doing network I/O there would put a timeout in front of ordinary work. So a
//! refresh writes to the cache, and reads come from the cache.

use crate::state::AppState;
use apoc_apocrypha::gamedb::{self, ApocryphaGameDb, SUPPORTED_SCHEMA};
use apoc_domain::GameProfile;
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_storage::GameDbSource;

/// Where the profiles being used actually came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSourceView {
    /// `local-builtin` or `online-api`: what the setting asks for.
    pub selected: String,
    /// Whether published profiles are actually in use right now.
    pub online_in_effect: bool,
    /// How many games came from the service.
    pub published: usize,
    /// Unix seconds when profiles were last fetched, if ever.
    pub fetched_at: Option<i64>,
}

/// The profiles to use, from a store the caller has already locked.
///
/// **This, rather than [`effective_profiles`], is what a caller holding the
/// store lock must use.** `AppState::store` is a `std::sync::Mutex`, which is
/// not reentrant: locking it again on the same thread does not fail, it blocks
/// forever against itself. Taking the guard as an argument makes that mistake
/// impossible to write, which matters because when it was possible it was made
/// — `build_context` did exactly this, and every Apply deadlocked before
/// writing a single file.
pub fn profiles_from(store: &apoc_storage::Store) -> Vec<GameProfile> {
    let builtin = LocalBuiltin::new().all().unwrap_or_default();

    if store.game_db_source().unwrap_or(GameDbSource::LocalBuiltin) != GameDbSource::OnlineApi {
        return builtin;
    }

    let cached = store
        .cached_profiles(SUPPORTED_SCHEMA as i64)
        .unwrap_or_default();

    let mut merged = builtin;
    for (_, document) in cached {
        // A document that will not parse is skipped, not merged in part. Half a
        // profile is worse than the bundled one it would have replaced.
        let Ok(profile) = gamedb::parse_document(&document) else {
            continue;
        };
        match merged.iter_mut().find(|g| g.id == profile.id) {
            Some(existing) => *existing = profile,
            None => merged.push(profile),
        }
    }
    merged
}

/// The profiles to use, locking the store to find out.
///
/// For callers that do not already hold it. One that does must use
/// [`profiles_from`] instead.
pub fn effective_profiles(state: &AppState) -> Vec<GameProfile> {
    let Ok(store) = state.store.lock() else {
        return LocalBuiltin::new().all().unwrap_or_default();
    };
    profiles_from(&store)
}

/// One profile, from a store the caller has already locked.
pub fn profile_from(store: &apoc_storage::Store, game_id: &str) -> Option<GameProfile> {
    profiles_from(store).into_iter().find(|g| g.id == game_id)
}

/// One profile, or the bundled one when the service has nothing to say.
pub fn effective_profile(state: &AppState, game_id: &str) -> Option<GameProfile> {
    effective_profiles(state)
        .into_iter()
        .find(|g| g.id == game_id)
}

/// What is in force, for the interface to state plainly.
///
/// Worth showing because an app that has silently fallen back looks exactly
/// like one that is working, right until somebody wonders why the profile they
/// published has not arrived.
pub fn source_view(state: &AppState) -> ProfileSourceView {
    let Ok(store) = state.store.lock() else {
        return ProfileSourceView {
            selected: GameDbSource::LocalBuiltin.as_str().to_string(),
            online_in_effect: false,
            published: 0,
            fetched_at: None,
        };
    };
    let selected = store.game_db_source().unwrap_or(GameDbSource::LocalBuiltin);
    let cached = store
        .cached_profiles(SUPPORTED_SCHEMA as i64)
        .unwrap_or_default();
    let fetched_at = store.profiles_fetched_at().unwrap_or(None);

    ProfileSourceView {
        selected: selected.as_str().to_string(),
        online_in_effect: selected == GameDbSource::OnlineApi && !cached.is_empty(),
        published: cached.len(),
        fetched_at,
    }
}

/// Fetch the published profiles and cache them.
///
/// Returns what happened in words, because this is only ever called by somebody
/// pressing a button and waiting to be told.
pub fn refresh(state: &AppState, app_version: &str) -> Result<String, String> {
    let client = ApocryphaGameDb::new(Default::default(), app_version);
    let documents = client.fetch_documents()?;

    let rows: Vec<(String, String, i64)> = documents
        .into_iter()
        .map(|(id, document, schema)| (id, document, schema as i64))
        .collect();
    let count = rows.len();

    let store = state
        .store
        .lock()
        .map_err(|_| "state poisoned".to_string())?;
    store
        .put_cached_profiles(&rows)
        .map_err(|e| format!("the profiles could not be saved: {e}"))?;

    Ok(match count {
        0 => "The service has no game profiles to publish yet.".to_string(),
        1 => "1 game profile updated.".to_string(),
        n => format!("{n} game profiles updated."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_storage::Store;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;

    /// The regression. `build_context` held the store lock and asked `AppState`
    /// for the profile in force, which locked it again on the same thread.
    ///
    /// A non-reentrant mutex does not report that: it blocks, holding the lock
    /// it is waiting for, so the only way to assert its absence is to bound the
    /// time. Applying deadlocked at "Preparing" before a single file was
    /// written, and no test could have failed on it, because there was no
    /// return value to be wrong.
    #[test]
    fn profiles_can_be_read_while_the_store_is_already_locked() {
        static STORE: Mutex<Option<Mutex<Store>>> = Mutex::new(None);
        *STORE.lock().unwrap() = Some(Mutex::new(Store::open_in_memory().unwrap()));

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outer = STORE.lock().unwrap();
            let store = outer.as_ref().unwrap();
            // Exactly the shape `build_context` has: hold the guard, then read
            // the profile in force through it.
            let guard = store.lock().unwrap();
            let profiles = profiles_from(&guard);
            let _ = tx.send(profiles.len());
        });

        let count = rx.recv_timeout(Duration::from_secs(10)).expect(
            "reading the profiles in force while holding the store lock must not block: \
             this is the deadlock that made every Apply hang",
        );
        assert!(
            count >= 6,
            "the bundled profiles are the floor, got {count}"
        );
    }
}
