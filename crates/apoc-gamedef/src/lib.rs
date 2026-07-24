//! The game-definition plugin system.
//!
//! Games are loaded as declarative [`GameProfile`] documents. The
//! [`GameDatabaseSource`] port abstracts *where* profiles come from; the
//! [`LocalBuiltin`] adapter ships a bundled set (the default), and a future
//! `ApocryphaApi` adapter will fetch them from the platform: the "Game Database
//! Source" settings toggle just swaps the implementation.

use apoc_domain::GameProfile;
use thiserror::Error;

/// Bundled TOML profiles compiled into the binary. Adding a game = adding a file
/// here plus one `include_str!` line below.
const MONSTER_HUNTER_WILDS: &str = include_str!("../profiles/monster_hunter_wilds.toml");

#[derive(Debug, Error)]
pub enum GameDefError {
    #[error("failed to parse builtin game profile: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("no game profile found for id '{0}'")]
    NotFound(String),
}

/// A source of game definitions. Local-first today, API-capable tomorrow.
///
/// Implementations must be cheap to call repeatedly (cache internally if needed).
pub trait GameDatabaseSource {
    /// List all known game profiles.
    fn all(&self) -> Result<Vec<GameProfile>, GameDefError>;

    /// Look up a single profile by its stable id.
    fn get(&self, id: &str) -> Result<GameProfile, GameDefError> {
        self.all()?
            .into_iter()
            .find(|g| g.id == id)
            .ok_or_else(|| GameDefError::NotFound(id.to_string()))
    }
}

/// The default, offline source: profiles bundled into the application binary.
#[derive(Debug, Default, Clone)]
pub struct LocalBuiltin;

impl LocalBuiltin {
    pub fn new() -> Self {
        LocalBuiltin
    }

    /// The raw bundled TOML documents. Kept separate so tests can assert every
    /// shipped profile parses.
    fn raw_profiles() -> &'static [&'static str] {
        &[MONSTER_HUNTER_WILDS]
    }
}

impl GameDatabaseSource for LocalBuiltin {
    fn all(&self) -> Result<Vec<GameProfile>, GameDefError> {
        Self::raw_profiles()
            .iter()
            .map(|raw| toml::from_str::<GameProfile>(raw).map_err(GameDefError::from))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{Engine, LoadOrderPolicy, LoaderKind};

    #[test]
    fn every_builtin_profile_parses() {
        let src = LocalBuiltin::new();
        let all = src.all().expect("builtin profiles must parse");
        assert!(!all.is_empty());
    }

    #[test]
    fn mhw_wilds_profile_is_correct() {
        let src = LocalBuiltin::new();
        let g = src.get("monster-hunter-wilds").expect("mhw present");

        assert_eq!(g.name, "Monster Hunter Wilds");
        assert_eq!(g.engine, Engine::ReEngine);
        assert_eq!(g.detection.steam_app_id, 2246340);
        assert_eq!(g.load_order, LoadOrderPolicy::Priority);
        assert!(g.case_sensitive, "RE Engine paths are case-sensitive on Linux");

        // Deploy targets cover both real payload roots seen in the sample mod.
        assert_eq!(g.target_for("natives"), Some("natives"));
        assert_eq!(g.target_for("reframework"), Some("reframework"));

        // Loader provisioning for Proton is present and correct.
        let loader = g.loader.as_ref().expect("REFramework loader defined");
        assert_eq!(loader.kind, LoaderKind::DllProxy);
        assert_eq!(loader.proxy_dll.as_deref(), Some("dinput8.dll"));
        assert!(loader.proton.requires_prefix_write);
        assert_eq!(loader.proton.wine_dll_overrides.as_deref(), Some("dinput8=n,b"));
    }

    #[test]
    fn unknown_game_is_not_found() {
        let src = LocalBuiltin::new();
        assert!(matches!(
            src.get("does-not-exist"),
            Err(GameDefError::NotFound(_))
        ));
    }
}
