//! The shapes RE Engine mods ship in, run against *every* RE Engine profile.
//!
//! Deliberately not one test file per game. Phase 3's claim is that a game on
//! an engine already supported costs a document and nothing else, and a battery
//! that enumerates the profiles is that claim made mechanical: adding a fourth
//! RE Engine title puts it through all of this without a line of test code.
//!
//! Every expectation is derived from the profile under test rather than written
//! out — the pak chain name in particular, because Wilds and the titles after
//! it disagree about it and a literal here would only ever assert one of them.

use apoc_domain::{DeployRoot, Engine, GameProfile, InstallerModel, ModBundle};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_modengine::GameRules;
use std::io::Write;

/// Every shipped RE Engine profile, with its rules.
///
/// Fails loudly on an empty set: a battery that silently tests nothing is worse
/// than no battery, because it reports success.
fn re_engine_profiles() -> Vec<(GameProfile, GameRules)> {
    let all = LocalBuiltin::new().all().expect("builtin profiles parse");
    let found: Vec<_> = all
        .into_iter()
        .filter(|g| g.engine == Engine::ReEngine)
        .map(|g| {
            let rules = GameRules::from_profile(&g);
            (g, rules)
        })
        .collect();
    assert!(
        found.len() >= 2,
        "expected at least two RE Engine profiles, found {}",
        found.len()
    );
    found
}

fn analyze(entries: &[(&str, &[u8])], rules: &GameRules) -> ModBundle {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mod.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    apoc_modengine::analyze_archive_with(&path, rules).unwrap()
}

fn dests(b: &ModBundle) -> Vec<String> {
    let mut out: Vec<String> = b
        .deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect();
    out.sort();
    out
}

#[test]
fn loose_files_under_natives_are_installed_where_the_engine_reads_them() {
    for (g, rules) in re_engine_profiles() {
        let b = analyze(
            &[
                ("natives/STM/wp/mod.mesh", b"M"),
                ("natives/STM/wp/mod.tex", b"T"),
            ],
            &rules,
        );
        assert_eq!(
            b.installer_model,
            InstallerModel::FlatNatives,
            "{}: a loose natives dump is not FlatNatives",
            g.id
        );
        assert_eq!(
            dests(&b),
            vec!["natives/STM/wp/mod.mesh", "natives/STM/wp/mod.tex"],
            "{}",
            g.id
        );
    }
}

#[test]
fn a_script_mod_lands_under_reframework() {
    for (g, rules) in re_engine_profiles() {
        let b = analyze(&[("reframework/autorun/thing.lua", b"L")], &rules);
        assert_eq!(
            b.installer_model,
            InstallerModel::ReframeworkOnly,
            "{}: a script-only mod is not ReframeworkOnly",
            g.id
        );
        assert_eq!(dests(&b), vec!["reframework/autorun/thing.lua"], "{}", g.id);
    }
}

#[test]
fn an_archive_packed_from_the_inside_out_still_imports() {
    // Authors zip the *inside* of a payload root often enough that treating it
    // as "0 files to install" would be a bug report every week.
    for (g, rules) in re_engine_profiles() {
        assert_eq!(
            dests(&analyze(&[("STM/wp/mod.mesh", b"M")], &rules)),
            vec!["natives/STM/wp/mod.mesh"],
            "{}: a bare STM/ was not rewrapped under natives/",
            g.id
        );
        assert_eq!(
            dests(&analyze(&[("autorun/thing.lua", b"L")], &rules)),
            vec!["reframework/autorun/thing.lua"],
            "{}: a bare autorun/ was not rewrapped under reframework/",
            g.id
        );
    }
}

#[test]
fn windows_casing_is_folded_back_onto_the_path_the_engine_reads() {
    // RE Engine resolves `natives/STM` with that exact casing. Linux is
    // case-sensitive, so a Windows-authored `natives/stm` would otherwise
    // create a second tree the game never looks at, and the mod would install
    // perfectly and do nothing.
    for (g, rules) in re_engine_profiles() {
        assert_eq!(
            dests(&analyze(&[("natives/stm/wp/mod.mesh", b"M")], &rules)),
            vec!["natives/STM/wp/mod.mesh"],
            "{}: casing was not canonicalised",
            g.id
        );
    }
}

#[test]
fn a_standalone_pak_is_accepted_and_named_into_this_games_own_chain() {
    // The one place these games genuinely disagree. Wilds interposes a
    // `sub_000` archive and patches above it; the titles after it patch the
    // base chunk directly. A pak named into the wrong chain deploys without
    // error and is never loaded, so the expectation is read from the profile.
    for (g, rules) in re_engine_profiles() {
        let b = analyze(
            &[
                ("Cool Mod/modinfo.ini", b"name=Cool Mod\nversion=1.0\n"),
                ("Cool Mod/cool.pak", b"PAK"),
            ],
            &rules,
        );
        let opt = b.options().next().unwrap();
        assert!(
            opt.deployable,
            "{}: a pak-only mod must be installable",
            g.id
        );
        assert_eq!(opt.payload.len(), 1, "{}", g.id);
        assert_eq!(
            opt.payload[0].root,
            DeployRoot::Pak,
            "{}: the pak was not routed into the patch chain",
            g.id
        );

        let chain = g
            .pak_chain
            .as_ref()
            .unwrap_or_else(|| panic!("{}: an RE Engine game needs a pak chain", g.id));
        let first = chain.filename(chain.start_index);
        assert!(
            first.ends_with(".pak") && first.contains("re_chunk_000"),
            "{}: implausible chain name {first}",
            g.id
        );
        assert_eq!(
            chain.index_of(&first),
            Some(chain.start_index),
            "{}: the chain name does not round-trip, so the next index would be misread",
            g.id
        );
    }
}

#[test]
fn a_bare_loader_dll_is_recognised_and_placed_where_the_profile_says() {
    // REFramework's release zip is exactly this shape, and it is how a Linux
    // user actually gets a loader into a Proton prefix.
    for (g, rules) in re_engine_profiles() {
        let loader = g
            .loader
            .as_ref()
            .unwrap_or_else(|| panic!("{}: no loader declared", g.id));
        let proxy = loader.proxy_dll.as_deref().expect("a dll-proxy loader");
        let file_name = proxy.rsplit('/').next().unwrap();

        let b = analyze(
            &[
                (file_name, b"MZ fake loader"),
                ("reframework_revision.txt", b"revision 1234"),
            ],
            &rules,
        );
        assert_eq!(
            b.installer_model,
            InstallerModel::Loader,
            "{}: a bare proxy DLL is not a Loader archive",
            g.id
        );
        assert_eq!(
            dests(&b),
            vec![proxy.to_string()],
            "{}: only the declared proxy deploys, and to the path the profile names",
            g.id
        );
    }
}

#[test]
fn a_game_that_declares_none_of_this_installs_none_of_it() {
    // The negative half. Every assertion above would also pass if the engine
    // simply installed whatever it found, so this drives the same archives
    // through rules that declare no roots, no loader and no chain.
    let bare = GameRules {
        payload_roots: Vec::new(),
        root_files: Vec::new(),
        accepts_pak: false,
        rewrap: Vec::new(),
        rewrap_extensions: Vec::new(),
        canonical_case: Vec::new(),
        formats: Vec::new(),
        fomod_dest_prefix: String::new(),
        plugin_extensions: Vec::new(),
        manages_plugin_list: false,
        root_folder: None,
        root_patterns: Vec::new(),
    };

    let loader = analyze(&[("dinput8.dll", b"MZ")], &bare);
    assert_eq!(loader.installer_model, InstallerModel::Unknown);
    assert_eq!(
        loader.deployable_options().count(),
        0,
        "a game with no declared loader must not install DLLs"
    );

    let pak = analyze(
        &[
            ("Cool Mod/modinfo.ini", b"name=Cool Mod\n"),
            ("Cool Mod/cool.pak", b"PAK"),
        ],
        &bare,
    );
    assert_eq!(
        pak.deployable_options().count(),
        0,
        "a game with no pak chain must not install a pak blindly"
    );
}
