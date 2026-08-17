//! Mods whose files belong beside the game executable rather than under a
//! payload root.
//!
//! Script extenders, ENB, ReShade, DXVK. Every case here is taken from how a
//! real one is packaged, because the failure this guards against is not a crash
//! — it is an install that reports success, keeps the half of the mod that fits
//! the usual shape, and silently drops the half that is the actual mod.
//!
//! The shape that prompted all of it: SKSE ships a loader `.exe`, a library
//! whose name carries the game version, and a `Data` folder beside them. Before
//! this, Apocrypha kept the `Data` folder.

use apoc_domain::{DeployRoot, SelectMode};
use apoc_modengine::GameRules;
use std::io::Write;
use std::path::Path;

/// Skyrim's rules, in the shape the shipped profile declares them.
fn creation_engine_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["Data".into()],
        canonical_case: vec!["Data".into()],
        plugin_extensions: vec!["esp".into(), "esm".into(), "esl".into()],
        root_folder: Some("Root".into()),
        root_patterns: vec![
            "*.exe".into(),
            "*.dll".into(),
            "*.bin".into(),
            "*.ini".into(),
        ],
        rewrap_extensions: vec![
            ("esp".into(), "Data".into()),
            ("esm".into(), "Data".into()),
            ("esl".into(), "Data".into()),
            ("bsa".into(), "Data".into()),
        ],
        ..GameRules::default()
    }
}

fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn analyze(entries: &[(&str, &[u8])], rules: &GameRules) -> apoc_domain::ModBundle {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mod.zip");
    zip_with(&path, entries);
    apoc_modengine::analyze_archive_with(&path, rules).expect("archive analyses")
}

/// Every file the default selection would install, as `destination` strings.
fn installed(bundle: &apoc_domain::ModBundle) -> Vec<String> {
    let selection = apoc_modengine::recommended_selection(bundle);
    let mut out: Vec<String> = bundle
        .options()
        .filter(|o| selection.contains(&o.id))
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect();
    out.sort();
    out
}

/// SKSE, as it actually ships.
const SKSE: &[(&str, &[u8])] = &[
    ("skse64_loader.exe", b"MZ"),
    ("skse64_1_6_1170.dll", b"MZ"),
    ("Data/Scripts/Actor.pex", b"pex"),
    ("Data/Scripts/Game.pex", b"pex"),
    ("skse64_readme.txt", b"read me"),
    ("skse64_whatsnew.txt", b"news"),
    ("src/common/IDebugLog.h", b"c++"),
];

#[test]
fn a_script_extender_installs_the_extender_and_not_only_its_scripts() {
    // The bug this whole file exists for. Keeping `Data/Scripts` and dropping
    // the loader gives a game that launches vanilla while the mod list says the
    // extender is installed.
    let bundle = analyze(SKSE, &creation_engine_rules());

    assert_eq!(
        installed(&bundle),
        vec![
            "Data/Scripts/Actor.pex",
            "Data/Scripts/Game.pex",
            "skse64_1_6_1170.dll",
            "skse64_loader.exe",
        ]
    );
}

#[test]
fn the_files_that_go_beside_the_executable_say_so() {
    let bundle = analyze(SKSE, &creation_engine_rules());
    let root: Vec<&str> = bundle
        .options()
        .flat_map(|o| o.payload.iter())
        .filter(|f| f.root == DeployRoot::GameRoot)
        .map(|f| f.game_rel_path.as_str())
        .collect();

    assert_eq!(root.len(), 2, "the loader and its library, nothing else");
    assert!(root.contains(&"skse64_loader.exe"));
}

#[test]
fn documentation_and_source_are_left_where_they_are() {
    // No exclusion list decides this. They match no pattern, so they are not
    // installed, and `src/` is not even at the root.
    let bundle = analyze(SKSE, &creation_engine_rules());
    let all = installed(&bundle);

    assert!(!all.iter().any(|p| p.contains("readme")), "{all:?}");
    assert!(!all.iter().any(|p| p.contains("whatsnew")), "{all:?}");
    assert!(!all.iter().any(|p| p.contains("IDebugLog")), "{all:?}");
}

#[test]
fn the_root_files_are_their_own_option_and_are_already_chosen() {
    // The advanced escape hatch: visible, skippable, and requiring no action to
    // install normally.
    let bundle = analyze(SKSE, &creation_engine_rules());
    let option = bundle
        .options()
        .find(|o| o.name == "Game folder files")
        .expect("root files get an option of their own");

    assert_eq!(option.select_mode, SelectMode::Stackable, "declinable");
    assert!(option.recommended, "and installed without being asked for");
    assert!(apoc_modengine::recommended_selection(&bundle).contains(&option.id));
}

#[test]
fn a_mod_packaged_for_root_builder_installs_without_repackaging() {
    // The convention Mod Organizer's Root Builder established. A large part of
    // the Nexus ecosystem is already packaged this way.
    let bundle = analyze(
        &[
            ("Root/dxgi.dll", b"MZ"),
            ("Root/enblocal.ini", b"[ENB]"),
            ("Data/textures/x.dds", b"dds"),
        ],
        &creation_engine_rules(),
    );

    assert_eq!(
        installed(&bundle),
        vec!["Data/textures/x.dds", "dxgi.dll", "enblocal.ini"]
    );
}

#[test]
fn a_data_folder_inside_the_root_folder_is_simply_data() {
    // Root Builder refuses the entire mod over this case. It needs no rule of
    // its own: the folder mirrors the game directory, so `Root/Data` is `Data`.
    let bundle = analyze(
        &[
            ("Root/Data/Interface/x.swf", b"swf"),
            ("Root/dxgi.dll", b"MZ"),
        ],
        &creation_engine_rules(),
    );

    assert_eq!(installed(&bundle), vec!["Data/Interface/x.swf", "dxgi.dll"]);
}

#[test]
fn a_mod_that_is_only_root_files_is_still_recognised() {
    // A bare ENB drop: no payload root anywhere in the archive. Before this it
    // classified as unknown and installed nothing.
    let bundle = analyze(
        &[
            ("d3d11.dll", b"MZ"),
            ("enbseries.ini", b"[ENB]"),
            ("readme.txt", b"read me"),
        ],
        &creation_engine_rules(),
    );

    assert_eq!(installed(&bundle), vec!["d3d11.dll", "enbseries.ini"]);
    assert_eq!(
        bundle.option_count(),
        1,
        "one option, not an empty Main beside it"
    );
}

#[test]
fn what_was_left_behind_is_reported() {
    // The failure mode this replaces was silence. A root file the profile's
    // patterns do not cover is still not installed — but it is now said.
    let bundle = analyze(
        &[
            ("Data/Scripts/x.pex", b"pex"),
            ("something.asi", b"MZ"),
            ("readme.txt", b"read me"),
        ],
        &creation_engine_rules(),
    );

    assert_eq!(bundle.unclaimed_root_files, vec!["something.asi"]);
    let notice = apoc_modengine::unclaimed_root_files_notice(&bundle).expect("says so");
    assert!(notice.contains("something.asi"), "{notice}");
    assert!(!notice.contains("readme"), "a readme is not news: {notice}");
}

#[test]
fn a_game_that_declares_no_root_files_is_unchanged() {
    // Five of the six games shipping today. An RE Engine archive with a stray
    // executable at its root must behave exactly as it did before.
    let re_engine = GameRules::default();
    let bundle = analyze(
        &[
            ("natives/STM/x.pak", b"pak"),
            ("tool.exe", b"MZ"),
            ("Root/thing.dll", b"MZ"),
        ],
        &re_engine,
    );

    assert_eq!(installed(&bundle), vec!["natives/STM/x.pak"]);
    assert!(
        bundle.unclaimed_root_files.is_empty(),
        "and it says nothing about files it was never going to take"
    );
}
