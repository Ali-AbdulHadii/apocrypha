//! Archive shapes that would otherwise import as "0 files to install".
//!
//! Every case here is a real packaging habit found in the Monster Hunter Wilds
//! mod ecosystem. The manager must recognise all of them, because a mod that
//! imports as empty looks broken to the user even though the archive is fine.

use apoc_domain::{DeployRoot, SelectMode};
use apoc_modengine::GameRules;
use std::io::Write;
use std::path::Path;

fn wilds_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["natives".into(), "reframework".into()],
        root_files: vec![("dinput8.dll".into(), "dinput8.dll".into())],
        accepts_pak: true,
        rewrap: vec![
            ("autorun".into(), "reframework".into()),
            ("plugins".into(), "reframework".into()),
            ("fonts".into(), "reframework".into()),
            ("STM".into(), "natives".into()),
        ],
        rewrap_extensions: Vec::new(),
        canonical_case: vec!["STM".into()],
        formats: Vec::new(),
        fomod_dest_prefix: String::new(),
        plugin_extensions: Vec::new(),
        manages_plugin_list: false,
        root_folder: None,
        root_patterns: Vec::new(),
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

fn analyze(entries: &[(&str, &[u8])]) -> apoc_domain::ModBundle {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mod.zip");
    zip_with(&path, entries);
    apoc_modengine::analyze_archive_with(&path, &wilds_rules()).unwrap()
}

fn all_dests(b: &apoc_domain::ModBundle) -> Vec<String> {
    b.deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect()
}

#[test]
fn lowercase_natives_stm_is_forced_to_engine_casing() {
    // RE Engine resolves `natives/STM`. On case-sensitive Linux a lowercase
    // archive would create a second tree the game never reads.
    let b = analyze(&[("natives/stm/art/model/x.mesh.241111606", b"M")]);
    assert_eq!(
        all_dests(&b),
        vec!["natives/STM/art/model/x.mesh.241111606"]
    );
}

#[test]
fn mixed_casing_archives_converge_on_one_tree() {
    let b = analyze(&[
        ("natives/stm/art/a.mesh.1", b"A"),
        ("natives/STM/art/b.mesh.1", b"B"),
        ("natives/Stm/art/c.mesh.1", b"C"),
    ]);
    let dests = all_dests(&b);
    assert_eq!(dests.len(), 3);
    assert!(
        dests.iter().all(|d| d.starts_with("natives/STM/")),
        "every file must land in the same tree: {dests:?}"
    );
}

#[test]
fn bare_autorun_folder_is_rewrapped_under_reframework() {
    // A very common packaging mistake: the author zipped the inside of
    // `reframework/`, so the archive starts at `autorun/`.
    let b = analyze(&[("autorun/my_script.lua", b"-- lua")]);
    assert_eq!(all_dests(&b), vec!["reframework/autorun/my_script.lua"]);
    assert!(b.deployable_options().next().is_some());
}

#[test]
fn bare_stm_folder_is_rewrapped_under_natives() {
    let b = analyze(&[("STM/art/model/x.mesh.1", b"M")]);
    assert_eq!(all_dests(&b), vec!["natives/STM/art/model/x.mesh.1"]);
}

#[test]
fn plugins_and_fonts_are_rewrapped_too() {
    let b = analyze(&[
        ("plugins/hb_draw.dll", b"DLL"),
        ("fonts/NotoSans.ttf", b"TTF"),
    ]);
    let dests = all_dests(&b);
    assert!(dests.contains(&"reframework/plugins/hb_draw.dll".to_string()));
    assert!(dests.contains(&"reframework/fonts/NotoSans.ttf".to_string()));
}

#[test]
fn reframework_script_mod_keeps_its_layout() {
    let b = analyze(&[
        ("reframework/autorun/crown_helper.lua", b"-- lua"),
        ("reframework/data/crown_helper.json", b"{}"),
    ]);
    let dests = all_dests(&b);
    assert!(dests.contains(&"reframework/autorun/crown_helper.lua".to_string()));
    assert!(dests.contains(&"reframework/data/crown_helper.json".to_string()));
}

#[test]
fn a_mixed_archive_installs_both_payload_roots() {
    // Armor mods routinely ship framework data next to game assets.
    let b = analyze(&[
        ("Mod/modinfo.ini", b"name=Mixed\nversion=v1\n"),
        ("Mod/natives/stm/art/x.mesh.1", b"M"),
        ("Mod/reframework/data/Framework/def.json", b"{}"),
    ]);
    let dests = all_dests(&b);
    assert!(dests.iter().any(|d| d.starts_with("natives/STM/")));
    assert!(dests.iter().any(|d| d.starts_with("reframework/data/")));
    assert_eq!(dests.len(), 2, "neither root may be dropped");
}

#[test]
fn dummy_mod_entries_are_never_installable() {
    // Fluffy's DummyMod marks a menu header that ships no files by design.
    // Presenting it as installable is exactly what produces a "0 files" install.
    let b = analyze(&[
        (
            "Header/modinfo.ini",
            b"name=--- Section ---\nDummyMod=True\n",
        ),
        ("Real/modinfo.ini", b"name=Real Mod\n"),
        ("Real/natives/stm/art/x.mesh.1", b"M"),
    ]);
    let dummy = b
        .options()
        .find(|o| o.name.contains("Section"))
        .expect("dummy option present");
    assert_eq!(dummy.select_mode, SelectMode::Info);
    assert!(!dummy.deployable);

    let real = b.options().find(|o| o.name == "Real Mod").unwrap();
    assert!(real.deployable, "the real option still installs");
}

#[test]
fn wrapper_folders_are_stripped_before_matching() {
    let b = analyze(&[("MonsterHunterWilds/natives/stm/art/x.mesh.1", b"M")]);
    assert_eq!(all_dests(&b), vec!["natives/STM/art/x.mesh.1"]);
}

#[test]
fn payload_roots_keep_their_deploy_root_classification() {
    let b = analyze(&[
        ("natives/stm/a.mesh.1", b"A"),
        ("autorun/b.lua", b"B"),
        ("dinput8.dll", b"L"),
    ]);
    let roots: Vec<DeployRoot> = b
        .deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.root.clone())
        .collect();
    assert!(roots.contains(&DeployRoot::Natives));
    assert!(roots.contains(&DeployRoot::Reframework));
    assert!(roots.contains(&DeployRoot::GameRoot));
}
