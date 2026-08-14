//! What counts as deployable content, derived from the active game definition.
//!
//! The mod engine is a mechanism; the *policy* of which paths are game content
//! comes from [`GameProfile`]. That keeps games as data: adding an engine with
//! different payload roots (or a different loader DLL) needs no code change here.

use apoc_domain::GameProfile;

/// Payload-recognition rules for one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRules {
    /// Top-level directories whose contents are deployable (`natives`, `reframework`).
    pub payload_roots: Vec<String>,
    /// Files deployable when they sit at the archive root: loader proxy DLLs
    /// such as REFramework's `dinput8.dll`, which ship with no folder at all.
    /// Each entry is `(archive file name, game-relative destination)`; the two
    /// differ when the loader lives in a subdirectory, as RED4ext's
    /// `winmm.dll` does at `bin/x64/winmm.dll`.
    pub root_files: Vec<(String, String)>,
    /// Whether a bare `.pak` beside `modinfo.ini` is installable content. True
    /// only for engines whose profile declares a patch chain to slot it into.
    pub accepts_pak: bool,
    /// Folders that appear at the root of a badly packed archive because the
    /// author zipped the *inside* of a payload root. Maps the folder name to
    /// the prefix that must be restored, e.g. `autorun` -> `reframework`.
    pub rewrap: Vec<(String, String)>,
    /// Exact casing to force on payload path components, e.g. `stm` -> `STM`.
    /// Windows-authored archives use random casing and Linux is case-sensitive,
    /// so without this the game silently ignores half a mod's files.
    pub canonical_case: Vec<String>,
    /// Archive formats this game's mods are expected to ship in. Empty means
    /// the profile has no opinion, which is treated as permission rather than
    /// refusal: see [`GameRules::supports_format`].
    pub formats: Vec<String>,
    /// Prefixed to every destination a FOMOD declares for this game.
    pub fomod_dest_prefix: String,
    /// Extensions whose files carry load order this application does not
    /// manage. Empty for engines with no such concept.
    pub plugin_extensions: Vec<String>,
}

impl Default for GameRules {
    /// RE Engine defaults, used when no game context is available.
    fn default() -> Self {
        GameRules {
            payload_roots: vec!["natives".to_string(), "reframework".to_string()],
            root_files: Vec::new(),
            accepts_pak: false,
            rewrap: Vec::new(),
            canonical_case: Vec::new(),
            formats: Vec::new(),
            fomod_dest_prefix: String::new(),
            plugin_extensions: Vec::new(),
        }
    }
}

impl GameRules {
    /// Derive rules from a game definition.
    pub fn from_profile(profile: &GameProfile) -> Self {
        let payload_roots = profile
            .deploy_targets
            .iter()
            .map(|t| t.source.clone())
            .collect::<Vec<_>>();

        // A loader's proxy DLL is content the manager must be able to install,
        // because that is how the loader reaches the game at all. The archive
        // ships it as a bare file name; the profile says where it belongs.
        //
        // Every proxy, not only the first: a game may have more than one, and
        // recognising one of two means an archive shipping the other imports as
        // zero files with nothing to explain why.
        let root_files = profile
            .loader
            .as_ref()
            .map(|l| {
                l.proxy_dlls()
                    .into_iter()
                    .map(|dest| {
                        let name = dest.rsplit('/').next().unwrap_or(dest).to_string();
                        (name, dest.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();

        GameRules {
            payload_roots: if payload_roots.is_empty() {
                GameRules::default().payload_roots
            } else {
                payload_roots
            },
            root_files,
            accepts_pak: profile.pak_chain.is_some(),
            rewrap: profile
                .rewrap
                .iter()
                .map(|r| (r.folder.clone(), r.prefix.clone()))
                .collect(),
            canonical_case: profile.canonical_case.clone(),
            formats: profile.formats.clone(),
            fomod_dest_prefix: profile
                .fomod
                .as_ref()
                .map(|f| f.dest_prefix.clone())
                .unwrap_or_default(),
            plugin_extensions: profile.plugin_extensions.clone(),
        }
    }

    /// True for a file that carries load order this application does not manage.
    pub fn is_plugin_file(&self, path: &str) -> bool {
        let Some(ext) = std::path::Path::new(path).extension() else {
            return false;
        };
        self.plugin_extensions
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e.as_str()))
    }

    /// Whether this game's mods are expected to ship in a given archive format.
    ///
    /// An empty list means the profile has said nothing, and that is read as
    /// permission rather than refusal. The alternative would make
    /// [`GameRules::default`] — which the CLI and every test fixture use —
    /// silently reject formats it has always accepted, and a mod that fails to
    /// install because a profile forgot to enumerate something is a worse
    /// outcome than one installed on a reasonable default.
    pub fn supports_format(&self, id: &str) -> bool {
        self.formats.is_empty() || self.formats.iter().any(|f| f.eq_ignore_ascii_case(id))
    }

    /// The prefix a stray root folder must be re-wrapped under, if any.
    /// `autorun/` at the archive root really means `reframework/autorun/`.
    pub fn rewrap_prefix(&self, root_name: &str) -> Option<&str> {
        self.rewrap
            .iter()
            .find(|(folder, _)| folder.eq_ignore_ascii_case(root_name))
            .map(|(_, prefix)| prefix.as_str())
    }

    /// Force the engine's expected casing on a path so a Windows-authored
    /// archive lands in one tree instead of two on a case-sensitive filesystem.
    pub fn canonicalize(&self, path: &str) -> String {
        if self.canonical_case.is_empty() {
            return path.to_string();
        }
        path.split('/')
            .map(|seg| {
                self.canonical_case
                    .iter()
                    .find(|c| c.eq_ignore_ascii_case(seg))
                    .map(|c| c.as_str())
                    .unwrap_or(seg)
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn is_payload_root(&self, name: &str) -> bool {
        self.payload_roots
            .iter()
            .any(|r| r.eq_ignore_ascii_case(name))
    }

    pub fn is_root_file(&self, name: &str) -> bool {
        self.root_file_dest(name).is_some()
    }

    /// Where a recognized root file is deployed, relative to the game directory.
    pub fn root_file_dest(&self, name: &str) -> Option<&str> {
        self.root_files
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, dest)| dest.as_str())
    }

    /// True for a standalone `.pak` mod archive, when this game supports them.
    pub fn is_pak_file(&self, name: &str) -> bool {
        self.accepts_pak
            && std::path::Path::new(name)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{
        ConflictScope, DeployTarget, Engine, LoadOrderPolicy, LoaderKind, LoaderSpec,
        ProtonLoaderSpec, SteamDetection,
    };

    fn profile() -> GameProfile {
        GameProfile {
            id: "g".into(),
            name: "G".into(),
            engine: Engine::ReEngine,
            nexus_domain: None,
            detection: SteamDetection {
                steam_app_id: 1,
                executable: None,
            },
            load_order: LoadOrderPolicy::Priority,
            conflict_scope: ConflictScope::PerRelativePath,
            case_sensitive: true,
            deploy_targets: vec![DeployTarget {
                source: "natives".into(),
                target: "natives".into(),
            }],
            formats: vec![],
            fomod: None,
            plugin_extensions: vec![],
            rewrap: vec![],
            canonical_case: vec!["STM".into()],
            pak_chain: Some(apoc_domain::PakChainSpec {
                pattern: "re_chunk_000.pak.sub_000.pak.patch_{n}.pak".into(),
                digits: 3,
                start_index: 1,
            }),
            loader: Some(LoaderSpec {
                name: "REFramework".into(),
                kind: LoaderKind::DllProxy,
                proxy_dll: Some("dinput8.dll".into()),
                also_provides: vec![],
                data_dirs: vec![],
                proton: ProtonLoaderSpec::default(),
            }),
        }
    }

    #[test]
    fn rules_come_from_the_game_definition() {
        let r = GameRules::from_profile(&profile());
        assert!(r.is_payload_root("natives"));
        assert!(
            !r.is_payload_root("reframework"),
            "not declared by this profile"
        );
        assert!(r.is_root_file("dinput8.dll"));
        assert!(
            r.is_root_file("DINPUT8.DLL"),
            "matching is case-insensitive"
        );
        assert!(!r.is_root_file("readme.txt"));
        assert!(r.is_pak_file("VerRBodyTextures-0-basic.pak"));
        assert!(!r.is_pak_file("notes.txt"));
    }

    #[test]
    fn casing_is_forced_to_what_the_engine_expects() {
        let r = GameRules::from_profile(&profile());
        // Windows-authored archives ship every casing; all must converge.
        assert_eq!(
            r.canonicalize("natives/stm/art/x.mesh.1"),
            "natives/STM/art/x.mesh.1"
        );
        assert_eq!(
            r.canonicalize("natives/Stm/art/x.mesh.1"),
            "natives/STM/art/x.mesh.1"
        );
        assert_eq!(
            r.canonicalize("natives/STM/art/x.mesh.1"),
            "natives/STM/art/x.mesh.1"
        );
    }

    #[test]
    fn a_profile_that_names_no_formats_permits_all_of_them() {
        // GameRules::default() backs the CLI and every test fixture. Reading an
        // empty list as "supports nothing" would silently refuse archives that
        // have always installed.
        let r = GameRules::default();
        assert!(r.supports_format("fomod"));
        assert!(r.supports_format("anything-at-all"));
    }

    #[test]
    fn a_profile_that_names_formats_answers_only_for_those() {
        let mut p = profile();
        p.formats = vec!["fluffy-aio".into(), "pak".into()];
        let r = GameRules::from_profile(&p);

        assert!(r.supports_format("fluffy-aio"));
        assert!(r.supports_format("FLUFFY-AIO"), "matching ignores casing");
        assert!(!r.supports_format("fomod"));
    }

    #[test]
    fn a_game_without_a_fomod_section_prefixes_nothing() {
        let r = GameRules::from_profile(&profile());
        assert_eq!(r.fomod_dest_prefix, "");
    }

    #[test]
    fn a_game_can_declare_where_its_fomod_destinations_are_rooted() {
        let mut p = profile();
        p.fomod = Some(apoc_domain::FomodSpec {
            dest_prefix: "Data".into(),
        });
        assert_eq!(GameRules::from_profile(&p).fomod_dest_prefix, "Data");
    }

    #[test]
    fn defaults_cover_re_engine_without_a_profile() {
        let r = GameRules::default();
        assert!(r.is_payload_root("natives") && r.is_payload_root("reframework"));
        assert!(r.root_files.is_empty());
        assert!(
            !r.is_pak_file("x.pak"),
            "pak mods need a declared patch chain, never a blind default"
        );
    }
}
