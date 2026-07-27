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
const CYBERPUNK_2077: &str = include_str!("../profiles/cyberpunk_2077.toml");

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
        &[MONSTER_HUNTER_WILDS, CYBERPUNK_2077]
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
    fn top_level_keys_are_not_swallowed_by_a_table() {
        // In TOML a bare key belongs to the most recent `[section]`, so a
        // `canonical_case = [...]` written below `[loader.proton]` parses fine
        // and is silently ignored. That is exactly how Wilds shipped with its
        // casing fix inert. The leaf structs now deny unknown fields, so a
        // misplaced key fails `all()` above; this asserts the values actually
        // arrived, which catches a key hoisted into a struct that still
        // accepts extras.
        let src = LocalBuiltin::new();
        for g in src.all().unwrap() {
            assert!(
                !g.formats.is_empty(),
                "{}: formats did not reach the top level",
                g.id
            );
            assert!(
                !g.canonical_case.is_empty(),
                "{}: canonical_case did not reach the top level",
                g.id
            );
            assert!(
                !g.rewrap.is_empty(),
                "{}: rewrap did not reach the top level",
                g.id
            );
            assert!(
                !g.deploy_targets.is_empty(),
                "{}: deploy_targets did not reach the top level",
                g.id
            );
        }
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
    fn cyberpunk_profile_is_correct() {
        let src = LocalBuiltin::new();
        let g = src.get("cyberpunk-2077").expect("cyberpunk present");

        assert_eq!(g.name, "Cyberpunk 2077");
        assert_eq!(g.engine, Engine::RedEngine);
        assert_eq!(g.detection.steam_app_id, 1091500);
        assert_eq!(g.nexus_domain.as_deref(), Some("cyberpunk2077"));

        // Every tree a Cyberpunk mod can ship content in must be deployable;
        // missing one silently drops that part of a mod.
        for root in ["archive", "mods", "r6", "red4ext", "bin", "engine"] {
            assert_eq!(g.target_for(root), Some(root), "root '{root}' not deployed");
        }

        // No patch chain: `.archive` files are loaded in place, unlike RE Engine
        // paks, which must be renamed into the chain to be seen at all.
        assert!(g.pak_chain.is_none());

        let loader = g.loader.as_ref().expect("RED4ext loader defined");
        assert_eq!(loader.kind, LoaderKind::DllProxy);
        assert_eq!(loader.proxy_dll.as_deref(), Some("bin/x64/winmm.dll"));
        assert_eq!(
            loader.proxy_dll_stem(),
            Some("winmm"),
            "the registry key is the module name, not the path"
        );
        // RED4ext and Cyber Engine Tweaks are separate proxies; both need an
        // override or half the modding stack does not load.
        assert_eq!(
            loader.dll_overrides(),
            vec![
                ("winmm".to_string(), "n,b".to_string()),
                ("version".to_string(), "n,b".to_string()),
            ]
        );
    }

    #[test]
    fn game_ids_and_steam_app_ids_are_unique() {
        // Two profiles sharing either one would make detection and every
        // per-game data directory ambiguous.
        let all = LocalBuiltin::new().all().unwrap();
        let mut ids: Vec<_> = all.iter().map(|g| g.id.clone()).collect();
        let mut apps: Vec<_> = all.iter().map(|g| g.detection.steam_app_id).collect();
        let (n_ids, n_apps) = (ids.len(), apps.len());
        ids.sort();
        ids.dedup();
        apps.sort();
        apps.dedup();
        assert_eq!(ids.len(), n_ids, "duplicate game id");
        assert_eq!(apps.len(), n_apps, "duplicate steam app id");
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
