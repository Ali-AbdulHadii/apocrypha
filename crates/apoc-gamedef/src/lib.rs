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
const SKYRIM_SPECIAL_EDITION: &str = include_str!("../profiles/skyrim_special_edition.toml");
const DRAGONS_DOGMA_2: &str = include_str!("../profiles/dragons_dogma_2.toml");

#[derive(Debug, Error)]
pub enum GameDefError {
    #[error("failed to parse builtin game profile: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("no game profile found for id '{0}'")]
    NotFound(String),
    #[error("game profile '{0}' declares load_order = \"explicit\" but no [plugin_list] section, so nothing could write the list it claims to manage")]
    ExplicitWithoutPluginList(String),
}

/// Refuse a profile whose claims and means disagree.
///
/// `load_order = "explicit"` is a promise that this application writes the
/// game's plugin list. `[plugin_list]` is what makes that possible. A profile
/// with the first and not the second announces management it cannot perform,
/// which is the exact failure the Skyrim profile spent three releases avoiding
/// by declaring `priority` and saying why in a comment.
///
/// Checked here rather than in `apoc-domain` because it is a rule about a
/// document being well-formed, and the domain crate describes what a profile
/// *is* rather than which ones are worth loading.
fn check_coherent(profile: &GameProfile) -> Result<(), GameDefError> {
    if profile.load_order == apoc_domain::LoadOrderPolicy::Explicit && profile.plugin_list.is_none()
    {
        return Err(GameDefError::ExplicitWithoutPluginList(profile.id.clone()));
    }
    Ok(())
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
        &[
            MONSTER_HUNTER_WILDS,
            CYBERPUNK_2077,
            SKYRIM_SPECIAL_EDITION,
            DRAGONS_DOGMA_2,
        ]
    }
}

impl GameDatabaseSource for LocalBuiltin {
    fn all(&self) -> Result<Vec<GameProfile>, GameDefError> {
        Self::raw_profiles()
            .iter()
            .map(|raw| {
                let profile = toml::from_str::<GameProfile>(raw)?;
                check_coherent(&profile)?;
                Ok(profile)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{Engine, LoadOrderPolicy, LoaderKind, PluginActivation};

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
    fn skyrim_se_profile_is_correct() {
        let src = LocalBuiltin::new();
        let g = src.get("skyrim-special-edition").expect("skyrim present");

        assert_eq!(g.name, "The Elder Scrolls V: Skyrim Special Edition");
        assert_eq!(g.engine, Engine::Creation);
        assert_eq!(g.detection.steam_app_id, 489830);
        assert_eq!(g.nexus_domain.as_deref(), Some("skyrimspecialedition"));

        // Everything a Skyrim mod installs lives under Data, and a FOMOD's
        // destinations are written relative to it.
        assert_eq!(g.target_for("Data"), Some("Data"));
        assert_eq!(
            g.fomod.as_ref().map(|f| f.dest_prefix.as_str()),
            Some("Data")
        );
        assert!(g.formats.iter().any(|f| f == "fomod"));

        // The most common Skyrim packaging: a bare `meshes/` at the archive
        // root, which means `Data/meshes/`. Without these an ordinary mod
        // imports as zero files.
        for folder in ["meshes", "textures", "scripts", "skse"] {
            assert!(
                g.rewrap
                    .iter()
                    .any(|r| r.folder == folder && r.prefix == "Data"),
                "{folder} is not rewrapped under Data"
            );
        }
    }

    #[test]
    fn skyrim_claims_to_manage_its_plugin_order_and_says_how() {
        // This assertion was the inverse until plugin management shipped: the
        // profile declared `priority` and a comment said why, so that the
        // manager admitted the gap rather than leaving someone to find it. The
        // claim is only allowed now because something honours it.
        let src = LocalBuiltin::new();
        let g = src.get("skyrim-special-edition").expect("skyrim present");

        assert_eq!(g.load_order, LoadOrderPolicy::Explicit);
        assert!(
            g.manages_plugin_list(),
            "the claim and the means to keep it arrive together"
        );
        assert_eq!(g.plugin_extensions, vec!["esp", "esm", "esl"]);

        let list = g.plugin_list.as_ref().expect("checked above");
        assert_eq!(list.dir, "Skyrim Special Edition");
        assert_eq!(list.plugins_file, "plugins.txt");
        assert_eq!(list.load_order_file.as_deref(), Some("loadorder.txt"));
        assert_eq!(list.activation, PluginActivation::Asterisk);
        assert!(
            list.implicit.iter().any(|p| p == "Skyrim.esm"),
            "every mod names the base game as a master"
        );
    }

    /// The rule that keeps the two halves together. A profile may not announce
    /// management it has given nothing the means to perform.
    #[test]
    fn a_profile_claiming_explicit_without_a_plugin_list_is_refused() {
        let mut profile = LocalBuiltin::new()
            .get("skyrim-special-edition")
            .expect("skyrim present");
        profile.plugin_list = None;

        let err = check_coherent(&profile).expect_err("this is not a loadable profile");
        assert!(matches!(err, GameDefError::ExplicitWithoutPluginList(id) if id == profile.id));
    }

    /// Games ordered by an integer per mod are unaffected and say nothing.
    #[test]
    fn the_re_engine_games_have_no_plugin_list_at_all() {
        let src = LocalBuiltin::new();
        for id in ["monster-hunter-wilds", "cyberpunk-2077"] {
            let g = src.get(id).expect("present");
            assert_eq!(g.load_order, LoadOrderPolicy::Priority, "{id}");
            assert!(g.plugin_list.is_none(), "{id}");
            assert!(!g.manages_plugin_list(), "{id}");
        }
    }

    #[test]
    fn skyrim_declares_no_loader_rather_than_the_wrong_one() {
        // SKSE is a separate launcher plus a DLL that nothing proxies, and
        // LoaderKind offers only `none` and `dll-proxy`. Declaring dll-proxy
        // would write a Wine override for a DLL that overrides nothing and
        // report "loader ready" when nothing had been set up.
        let src = LocalBuiltin::new();
        let g = src.get("skyrim-special-edition").expect("skyrim present");
        assert!(g.loader.is_none());
    }

    #[test]
    fn mhw_wilds_profile_is_correct() {
        let src = LocalBuiltin::new();
        let g = src.get("monster-hunter-wilds").expect("mhw present");

        assert_eq!(g.name, "Monster Hunter Wilds");
        assert_eq!(g.engine, Engine::ReEngine);
        assert_eq!(g.detection.steam_app_id, 2246340);
        assert_eq!(g.load_order, LoadOrderPolicy::Priority);
        assert!(
            g.case_sensitive,
            "RE Engine paths are case-sensitive on Linux"
        );

        // Deploy targets cover both real payload roots seen in the sample mod.
        assert_eq!(g.target_for("natives"), Some("natives"));
        assert_eq!(g.target_for("reframework"), Some("reframework"));

        // Loader provisioning for Proton is present and correct.
        let loader = g.loader.as_ref().expect("REFramework loader defined");
        assert_eq!(loader.kind, LoaderKind::DllProxy);
        assert_eq!(loader.proxy_dll.as_deref(), Some("dinput8.dll"));
        assert!(loader.proton.requires_prefix_write);
        assert_eq!(
            loader.proton.wine_dll_overrides.as_deref(),
            Some("dinput8=n,b")
        );
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
    fn dragons_dogma_2_profile_is_correct() {
        let src = LocalBuiltin::new();
        let g = src
            .get("dragons-dogma-2")
            .expect("dragon's dogma 2 present");

        assert_eq!(g.name, "Dragon's Dogma 2");
        assert_eq!(g.engine, Engine::ReEngine);
        assert_eq!(g.detection.steam_app_id, 2054970);
        assert_eq!(g.nexus_domain.as_deref(), Some("dragonsdogma2"));
        assert_eq!(g.load_order, LoadOrderPolicy::Priority);
        assert!(
            g.case_sensitive,
            "RE Engine paths are case-sensitive on Linux"
        );

        assert_eq!(g.target_for("natives"), Some("natives"));
        assert_eq!(g.target_for("reframework"), Some("reframework"));

        // The one field that is not Wilds': Dragon's Dogma 2 patches the base
        // chunk directly, where Wilds interposes a `sub_000` archive first. A
        // pak named into the wrong chain deploys without error and is never
        // loaded, so this asserts the name a user would see rather than the
        // template that produces it.
        let chain = g.pak_chain.as_ref().expect("standalone paks are supported");
        assert_eq!(chain.filename(1), "re_chunk_000.pak.patch_001.pak");
        assert_eq!(chain.index_of("re_chunk_000.pak.patch_007.pak"), Some(7));

        let loader = g.loader.as_ref().expect("REFramework loader defined");
        assert_eq!(loader.kind, LoaderKind::DllProxy);
        assert_eq!(loader.proxy_dll.as_deref(), Some("dinput8.dll"));
        assert!(loader.proton.requires_prefix_write);
        assert_eq!(
            loader.proton.wine_dll_overrides.as_deref(),
            Some("dinput8=n,b")
        );

        // A bare `STM/` or `autorun/` at an archive root is how RE Engine mods
        // are packed often enough that missing either imports the mod as zero
        // files.
        for (folder, prefix) in [("STM", "natives"), ("autorun", "reframework")] {
            assert!(
                g.rewrap
                    .iter()
                    .any(|r| r.folder == folder && r.prefix == prefix),
                "{folder} is not rewrapped under {prefix}"
            );
        }
    }

    #[test]
    fn the_re_engine_profiles_agree_on_everything_except_identity() {
        // The claim Phase 3 makes is that a game on an engine already supported
        // costs a document and nothing else. That is only true if two profiles
        // on the same engine differ solely in who they are, so this asserts it
        // rather than leaving it to be believed. A new RE Engine game that has
        // to disagree here is a finding: either the schema is missing something
        // or the game is not the cheap case it looked like.
        let all = LocalBuiltin::new().all().unwrap();
        let wilds = all.iter().find(|g| g.id == "monster-hunter-wilds").unwrap();
        let dd2 = all.iter().find(|g| g.id == "dragons-dogma-2").unwrap();

        assert_eq!(wilds.engine, dd2.engine);
        assert_eq!(wilds.load_order, dd2.load_order);
        assert_eq!(wilds.conflict_scope, dd2.conflict_scope);
        assert_eq!(wilds.case_sensitive, dd2.case_sensitive);
        assert_eq!(wilds.canonical_case, dd2.canonical_case);
        assert_eq!(wilds.formats, dd2.formats);
        assert_eq!(wilds.deploy_targets, dd2.deploy_targets);
        assert_eq!(wilds.rewrap, dd2.rewrap);
        assert_eq!(wilds.loader, dd2.loader);

        // Identity, and the patch chain, are what a new game is allowed to
        // change without that meaning anything.
        assert_ne!(wilds.id, dd2.id);
        assert_ne!(wilds.detection.steam_app_id, dd2.detection.steam_app_id);
        assert_ne!(wilds.pak_chain, dd2.pak_chain);
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
