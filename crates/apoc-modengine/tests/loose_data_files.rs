//! Archives that simply *are* the payload folder.
//!
//! A great many Skyrim mods ship the contents of `Data` with no `Data` around
//! them: SkyUI is an `.esp` and a `.bsa` at the archive root and nothing else.
//! There is no directory anywhere in the archive, so nothing about its shape
//! says where the files go — only the extensions do.
//!
//! Before this, such an archive classified as `Unknown` and installed nothing,
//! which for the most-downloaded interface mod on the game is the whole mod
//! silently missing.

use apoc_modengine::GameRules;
use std::io::Write;
use std::path::Path;

/// Skyrim's rules, in the shape the shipped profile declares them.
fn creation_engine_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["Data".into()],
        canonical_case: vec!["Data".into()],
        plugin_extensions: vec!["esp".into(), "esm".into(), "esl".into()],
        rewrap: vec![
            ("meshes".into(), "Data".into()),
            ("textures".into(), "Data".into()),
        ],
        rewrap_extensions: vec![
            ("esp".into(), "Data".into()),
            ("esm".into(), "Data".into()),
            ("esl".into(), "Data".into()),
            ("bsa".into(), "Data".into()),
        ],
        root_patterns: vec!["*.exe".into(), "*.dll".into()],
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

/// Every file the default selection would install.
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

/// SkyUI, entry for entry.
const SKYUI: &[(&str, &[u8])] = &[("SkyUI_SE.bsa", b"BSA\0"), ("SkyUI_SE.esp", b"TES4")];

#[test]
fn an_archive_that_is_the_data_folder_installs_into_data() {
    let bundle = analyze(SKYUI, &creation_engine_rules());

    assert_eq!(
        installed(&bundle),
        vec!["Data/SkyUI_SE.bsa", "Data/SkyUI_SE.esp"],
        "the archive root is the Data folder, so that is where its files go"
    );
}

#[test]
fn such_an_archive_is_classified_rather_than_given_up_on() {
    // It used to be `Unknown`, which installs nothing at all: with no directory
    // anywhere in the archive there was nothing for the shape checks to see.
    let bundle = analyze(SKYUI, &creation_engine_rules());

    assert_eq!(
        bundle.installer_model,
        apoc_domain::InstallerModel::LooseRoots
    );
    assert_eq!(bundle.deployable_options().count(), 1);
}

#[test]
fn nothing_is_reported_as_left_behind() {
    // The bug as the user met it: a warning naming the two files and telling
    // them to copy the mod into the game directory by hand. They are installed
    // now, so there is nothing to say.
    let bundle = analyze(SKYUI, &creation_engine_rules());

    assert!(
        bundle.unclaimed_root_files.is_empty(),
        "{:?}",
        bundle.unclaimed_root_files
    );
    assert!(apoc_modengine::unclaimed_root_files_notice(&bundle).is_none());
}

#[test]
fn a_plugin_already_under_data_is_left_where_it_is() {
    // The rule is about a *missing* folder. A file that already names its
    // directory must not be wrapped a second time into `Data/Data`.
    let bundle = analyze(
        &[("Data/Correct.esp", b"TES4"), ("Data/meshes/x.nif", b"nif")],
        &creation_engine_rules(),
    );

    assert_eq!(
        installed(&bundle),
        vec!["Data/Correct.esp", "Data/meshes/x.nif"]
    );
}

#[test]
fn a_plugin_inside_a_rewrapped_folder_keeps_that_folders_prefix() {
    // `meshes/` already restores `Data/`. The extension rule must not fight it
    // by claiming the file for itself and dropping the folder.
    let bundle = analyze(
        &[("meshes/thing.nif", b"nif"), ("textures/t.dds", b"dds")],
        &creation_engine_rules(),
    );

    assert_eq!(
        installed(&bundle),
        vec!["Data/meshes/thing.nif", "Data/textures/t.dds"]
    );
}

#[test]
fn the_two_kinds_of_loose_file_go_to_their_own_places() {
    // A mod shipping both: an extender DLL belongs beside the executable, a
    // plugin belongs under Data. Neither rule may take the other's files.
    let mut rules = creation_engine_rules();
    rules.root_patterns = vec!["*.dll".into()];

    let bundle = analyze(
        &[("some_extender.dll", b"MZ"), ("Mod.esp", b"TES4")],
        &rules,
    );

    assert_eq!(
        installed(&bundle),
        vec!["Data/Mod.esp", "some_extender.dll"]
    );
}

#[test]
fn a_game_that_declares_no_extensions_is_unchanged() {
    // Every RE Engine profile. A stray file at an archive root must not start
    // being installed somewhere on their behalf.
    let bundle = analyze(
        &[("natives/STM/x.pak", b"pak"), ("Readme.esp", b"TES4")],
        &GameRules::default(),
    );

    assert_eq!(installed(&bundle), vec!["natives/STM/x.pak"]);
}
