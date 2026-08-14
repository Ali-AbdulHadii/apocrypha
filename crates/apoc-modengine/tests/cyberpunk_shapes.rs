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
fn a_redmod_with_the_trees_redmod_actually_defines_survives_whole() {
    // REDmod is CDPR's own format and its folder is a small specification:
    // `info.json` names the mod, and the loader looks for archives, tweaks,
    // scripts and sounds in fixed places beside it. Losing any one of them
    // leaves a mod REDmod will load and then find half of.
    let b = analyze(&[
        ("mods/car_dealer/info.json", b"{\"name\":\"car_dealer\"}"),
        ("mods/car_dealer/archives/car_dealer.archive", b"A"),
        ("mods/car_dealer/archives/car_dealer.archive.xl", b"X"),
        ("mods/car_dealer/tweaks/vehicles.yaml", b"T"),
        ("mods/car_dealer/scripts/core/dealer.script", b"S"),
        ("mods/car_dealer/customSounds/horn.wav", b"W"),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "mods/car_dealer/archives/car_dealer.archive",
            "mods/car_dealer/archives/car_dealer.archive.xl",
            "mods/car_dealer/customSounds/horn.wav",
            "mods/car_dealer/info.json",
            "mods/car_dealer/scripts/core/dealer.script",
            "mods/car_dealer/tweaks/vehicles.yaml",
        ]
    );
    // `scripts` and `tweaks` are in the casing list and also rewrap folders, but
    // inside a REDmod they are already where they belong. Neither rule may reach
    // in and move them out to `r6/`.
    assert!(
        dests.iter().all(|d| d.starts_with("mods/car_dealer/")),
        "a REDmod's own subtrees escaped its folder: {dests:?}"
    );
    // customSounds is the author-facing spelling REDmod defines, and nothing in
    // the casing list may flatten it.
    assert!(dests.contains(&"mods/car_dealer/customSounds/horn.wav".to_string()));
}

#[test]
fn an_archive_that_is_only_the_engine_tree_imports() {
    // redscript ships exactly this and nothing else. No test had reached
    // `engine/` at all before, which is the tree holding the compiler and the
    // one shipped config file a mod replaces.
    let b = analyze(&[
        ("engine/config/base/scripts.ini", b"I"),
        ("engine/tools/scc.exe", b"E"),
        ("engine/tools/scc_lib.dll", b"D"),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "engine/config/base/scripts.ini",
            "engine/tools/scc.exe",
            "engine/tools/scc_lib.dll",
        ]
    );
    assert_eq!(b.installer_model, InstallerModel::LooseRoots);
}

#[test]
fn an_archive_that_is_only_the_bin_tree_imports() {
    // Cyber Engine Tweaks' release is this shape: everything under `bin/x64`,
    // including a proxy DLL. It must not be mistaken for a bare loader archive,
    // because it carries its own full paths and the loader model would flatten
    // them.
    let b = analyze(&[
        ("bin/x64/version.dll", b"V"),
        ("bin/x64/global.ini", b"I"),
        ("bin/x64/plugins/cyber_engine_tweaks.asi", b"A"),
        (
            "bin/x64/plugins/cyber_engine_tweaks/tweakdb/usedhashes.kark",
            b"K",
        ),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "bin/x64/global.ini",
            "bin/x64/plugins/cyber_engine_tweaks.asi",
            "bin/x64/plugins/cyber_engine_tweaks/tweakdb/usedhashes.kark",
            "bin/x64/version.dll",
        ]
    );
    assert_eq!(b.installer_model, InstallerModel::LooseRoots);
}

#[test]
fn a_deep_script_tree_arrives_with_its_shape_intact() {
    // A scripted mod is not a flat folder of files: redscript compiles a tree,
    // and the tree is how the mod's own imports resolve. Flattening or renaming
    // any level of it produces a mod that fails to compile at game start, with
    // an error naming a file the author never wrote.
    let b = analyze(&[
        ("r6/scripts/VirtualCarDealer/Utils.reds", b"U"),
        (
            "r6/scripts/VirtualCarDealer/core/CarDealer-System.reds",
            b"S",
        ),
        ("r6/scripts/VirtualCarDealer/UI/HubButton.reds", b"H"),
        (
            "r6/scripts/VirtualCarDealer/overrides/SetupNewTab.reds",
            b"O",
        ),
    ]);
    let mut dests = all_dests(&b);
    dests.sort();
    assert_eq!(
        dests,
        vec![
            "r6/scripts/VirtualCarDealer/UI/HubButton.reds",
            "r6/scripts/VirtualCarDealer/Utils.reds",
            "r6/scripts/VirtualCarDealer/core/CarDealer-System.reds",
            "r6/scripts/VirtualCarDealer/overrides/SetupNewTab.reds",
        ]
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
fn the_directories_frameworks_read_by_name_fold_to_one_casing() {
    // Not the game's own directories this time, but the ones its modding
    // frameworks open by an exact path. Two mods disagreeing about the casing of
    // `redsUserHints` produce two directories on Linux, and the compiler reads
    // whichever one it was told about, so the other mod's hints are simply not
    // there. The symptom is a mod that installs perfectly and does nothing.
    assert_eq!(
        all_dests(&analyze(&[("engine/Config/Base/scripts.ini", b"I")])),
        vec!["engine/config/base/scripts.ini"]
    );
    assert_eq!(
        all_dests(&analyze(&[("R6/Config/CyberCmd/scc.toml", b"T")])),
        vec!["r6/config/cybercmd/scc.toml"]
    );
    // The one entry that is not lowercase: proof this is a casing table rather
    // than a lowercasing pass, in both directions.
    assert_eq!(
        all_dests(&analyze(&[("r6/config/redsuserhints/mod.toml", b"T")])),
        vec!["r6/config/redsUserHints/mod.toml"]
    );
    assert_eq!(
        all_dests(&analyze(&[("r6/config/RedsUserHints/mod.toml", b"T")])),
        vec!["r6/config/redsUserHints/mod.toml"]
    );
    assert_eq!(
        all_dests(&analyze(&[(
            "bin/X64/Plugins/Cyber_Engine_Tweaks/Fonts/n.ttf",
            b"F"
        )])),
        vec!["bin/x64/plugins/cyber_engine_tweaks/fonts/n.ttf"]
    );
    assert_eq!(
        all_dests(&analyze(&[(
            "bin/x64/plugins/cyber_engine_tweaks/TweakDB/usedhashes.kark",
            b"K"
        )])),
        vec!["bin/x64/plugins/cyber_engine_tweaks/tweakdb/usedhashes.kark"]
    );
}

#[test]
fn a_folder_the_author_named_keeps_the_casing_they_gave_it() {
    // The other half of the rule. Nothing fixed reads these names, so folding
    // them would be inventing a convention on an author's behalf and would break
    // whichever mod spelled it the other way.
    assert_eq!(
        all_dests(&analyze(&[(
            "r6/scripts/VirtualCarDealer/UI/Hub.reds",
            b"R"
        )])),
        vec!["r6/scripts/VirtualCarDealer/UI/Hub.reds"]
    );
    // Vendored inside Cyber Engine Tweaks' own distribution, so there is no
    // second author to converge with and the list deliberately omits both.
    assert_eq!(
        all_dests(&analyze(&[(
            "bin/x64/plugins/cyber_engine_tweaks/scripts/IconGlyphs/icons.lua",
            b"L"
        )])),
        vec!["bin/x64/plugins/cyber_engine_tweaks/scripts/IconGlyphs/icons.lua"]
    );
    assert_eq!(
        all_dests(&analyze(&[("r6/scripts/MyMod/JSON/data.json", b"J")])),
        vec!["r6/scripts/MyMod/JSON/data.json"]
    );
}

#[test]
fn a_file_is_not_mistaken_for_the_directory_it_is_named_after() {
    // Cyber Engine Tweaks ships both `plugins/cyber_engine_tweaks.asi` and
    // `plugins/cyber_engine_tweaks/`. The casing list holds the directory name,
    // and matching is on whole segments, so the `.asi` beside it must come
    // through untouched. A prefix match here would rename a file into a
    // directory that already exists.
    assert_eq!(
        all_dests(&analyze(&[
            ("bin/x64/plugins/cyber_engine_tweaks.asi", b"A"),
            ("bin/x64/plugins/cyber_engine_tweaks/global.ini", b"I"),
        ])),
        vec![
            "bin/x64/plugins/cyber_engine_tweaks.asi",
            "bin/x64/plugins/cyber_engine_tweaks/global.ini",
        ]
    );
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

/// A known-wrong output, pinned so it is visible rather than discovered.
///
/// `canonical_case` entries are bare segments and match anywhere in a path, so
/// every generic word in the list is a global rename. The profile records that
/// as a limitation; this is what it costs. `mod` earns its place because
/// `Archive/PC/Mod/` is the commonest Windows-authored shape in this game, and
/// the same entry then renames an author's own `r6/scripts/Mod/` — a change
/// nobody asked for, in a directory that belongs to them.
///
/// Nothing breaks today: redscript compiles a tree and takes module names from
/// inside the files, not from the folder. It is still a rename we have no right
/// to make, and the same applies to `mods`, `base`, `tools`, `config`, `cache`,
/// `input`, `tweaks`, `scripts`, `fonts`, `plugins`, `pc` and `x64`.
///
/// The fix is anchoring entries to a path prefix, which changes the profile
/// schema, the order normalization applies rewrapping and casing in, and both
/// shipped games. That is deliberately not done here. When it is, this test
/// should fail, and the assertion below is the one to invert.
#[test]
fn an_author_folder_sharing_a_name_with_a_game_directory_is_renamed() {
    let b = analyze(&[("r6/scripts/Mod/Main.reds", b"module Mod")]);
    assert_eq!(
        all_dests(&b),
        vec!["r6/scripts/mod/Main.reds"],
        "if this now reads `Mod`, casing became prefix-anchored and the \
         limitation in the profile comment can be deleted"
    );
}

#[test]
fn documentation_only_archives_deploy_nothing() {
    // A readme is not content. Deploying it would put junk in the game folder.
    let b = analyze(&[("README.md", b"hi"), ("preview.png", b"PNG")]);
    assert!(all_dests(&b).is_empty());
    assert_eq!(b.installer_model, InstallerModel::Unknown);
}
