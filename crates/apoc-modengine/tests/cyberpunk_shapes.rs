//! Cyberpunk 2077 archive shapes, driven by the shipped game profile.
//!
//! These are the packaging habits of the REDengine ecosystem, and they are the
//! real test of whether the game-profile abstraction generalises: the rules
//! here are built from `cyberpunk_2077.toml` rather than written by hand, so a
//! profile that would import a mod as empty fails a test instead of a user.

use apoc_domain::{InstallerModel, ModBundle};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_modengine::GameRules;
use std::io::Write;
use std::path::Path;

fn cp77_rules() -> GameRules {
    let profile = LocalBuiltin::new()
        .get("cyberpunk-2077")
        .expect("cyberpunk profile ships");
    GameRules::from_profile(&profile)
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

fn analyze(entries: &[(&str, &[u8])]) -> ModBundle {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("mod.zip");
    zip_with(&path, entries);
    apoc_modengine::analyze_archive_with(&path, &cp77_rules()).unwrap()
}

fn all_dests(b: &ModBundle) -> Vec<String> {
    b.deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect()
}

#[test]
fn a_plain_archive_mod_deploys_into_the_mod_folder() {
    // The most common shape on Nexus: a single `.archive` under `archive/pc/mod`.
    let b = analyze(&[("archive/pc/mod/nicer_coat.archive", b"A")]);
    assert_eq!(all_dests(&b), vec!["archive/pc/mod/nicer_coat.archive"]);
    assert_eq!(b.installer_model, InstallerModel::LooseRoots);
}

#[test]
fn a_mod_spanning_four_trees_keeps_every_one() {
    // REDengine scatters one mod across unrelated trees. Dropping any of them
    // leaves a mod that installs "successfully" and then does nothing.
    let b = analyze(&[
        ("archive/pc/mod/thing.archive", b"A"),
        ("archive/pc/mod/thing.archive.xl", b"X"),
        ("r6/scripts/thing/thing.reds", b"R"),
        ("r6/tweaks/thing.yaml", b"T"),
        ("red4ext/plugins/thing/thing.dll", b"D"),
        (
            "bin/x64/plugins/cyber_engine_tweaks/mods/thing/init.lua",
            b"L",
        ),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "archive/pc/mod/thing.archive",
            "archive/pc/mod/thing.archive.xl",
            "bin/x64/plugins/cyber_engine_tweaks/mods/thing/init.lua",
            "r6/scripts/thing/thing.reds",
            "r6/tweaks/thing.yaml",
            "red4ext/plugins/thing/thing.dll",
        ]
    );
}

#[test]
fn a_redmod_keeps_its_own_folder() {
    let b = analyze(&[
        ("mods/thing/info.json", b"{}"),
        ("mods/thing/archives/thing.archive", b"A"),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec!["mods/thing/archives/thing.archive", "mods/thing/info.json"]
    );
}

#[test]
fn windows_casing_is_folded_back_into_the_real_tree() {
    // Linux is case-sensitive and REDengine's directories are lowercase, so
    // `Archive/PC/Mod` would otherwise create a tree the game never reads.
    let b = analyze(&[("Archive/PC/Mod/thing.archive", b"A")]);
    assert_eq!(all_dests(&b), vec!["archive/pc/mod/thing.archive"]);
}

#[test]
fn an_archive_packed_from_the_inside_out_still_imports() {
    // Authors zip the inside of a payload root often enough that treating this
    // as "0 files to install" would be a bug report every week.
    assert_eq!(
        all_dests(&analyze(&[("scripts/thing/thing.reds", b"R")])),
        vec!["r6/scripts/thing/thing.reds"]
    );
    assert_eq!(
        all_dests(&analyze(&[("pc/mod/thing.archive", b"A")])),
        vec!["archive/pc/mod/thing.archive"]
    );
    assert_eq!(
        all_dests(&analyze(&[("plugins/thing/thing.dll", b"D")])),
        vec!["red4ext/plugins/thing/thing.dll"]
    );
    assert_eq!(
        all_dests(&analyze(&[(
            "x64/plugins/cyber_engine_tweaks/mods/t/init.lua",
            b"L"
        )])),
        vec!["bin/x64/plugins/cyber_engine_tweaks/mods/t/init.lua"]
    );
}

#[test]
fn a_wrapper_folder_named_after_the_mod_is_stripped() {
    let b = analyze(&[("Nicer Coat v1.2/archive/pc/mod/coat.archive", b"A")]);
    assert_eq!(all_dests(&b), vec!["archive/pc/mod/coat.archive"]);
}

#[test]
fn the_bare_loader_dll_lands_beside_the_game_binary() {
    // RED4ext's proxy belongs at `bin/x64/winmm.dll`. Dropped at the game root
    // it is simply never loaded, and the mod stack silently does nothing.
    let b = analyze(&[("winmm.dll", b"MZ")]);
    assert_eq!(all_dests(&b), vec!["bin/x64/winmm.dll"]);
    assert_eq!(b.installer_model, InstallerModel::Loader);
}

#[test]
fn the_full_red4ext_release_keeps_its_own_layout() {
    // Shipped with the path already correct, it must not be re-derived.
    let b = analyze(&[
        ("bin/x64/winmm.dll", b"MZ"),
        ("red4ext/RED4ext.dll", b"MZ"),
        ("red4ext/plugins/.keep", b""),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "bin/x64/winmm.dll",
            "red4ext/RED4ext.dll",
            "red4ext/plugins/.keep"
        ]
    );
}

#[test]
fn documentation_only_archives_deploy_nothing() {
    // A readme is not content. Deploying it would put junk in the game folder.
    let b = analyze(&[("README.md", b"hi"), ("preview.png", b"PNG")]);
    assert!(all_dests(&b).is_empty());
    assert_eq!(b.installer_model, InstallerModel::Unknown);
}
