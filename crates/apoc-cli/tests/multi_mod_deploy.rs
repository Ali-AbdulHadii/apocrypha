//! Deploying a whole profile: several mods of different formats applied as one
//! journaled transaction.
//!
//! This is the regression test for the bug where a modded game launched with
//! almost nothing installed, because only the first enabled mod was deployed.

use apoc_deploy::{apply, dry_run, place::Ladder, rollback, DeployContext};
use apoc_domain::PakChainSpec;
use apoc_modengine::{GameRules, ModPlan};
use std::io::Write;
use std::path::Path;

fn wilds_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["natives".into(), "reframework".into()],
        root_files: vec![("dinput8.dll".into(), "dinput8.dll".into())],
        accepts_pak: true,
        rewrap: vec![],
        canonical_case: vec!["STM".into()],
        formats: Vec::new(),
        fomod_dest_prefix: String::new(),
        plugin_extensions: Vec::new(),
        manages_plugin_list: false,
    }
}

fn wilds_chain() -> PakChainSpec {
    PakChainSpec {
        pattern: "re_chunk_000.pak.sub_000.pak.patch_{n}.pak".into(),
        digits: 3,
        start_index: 1,
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

#[test]
fn every_enabled_mod_is_deployed_in_one_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let staging_root = tmp.path().join("staging");
    let game_dir = tmp.path().join("MonsterHunterWilds");
    std::fs::create_dir_all(&game_dir).unwrap();

    // The game already ships a patch chain up to 015, like a real install.
    for i in 1..=15u32 {
        std::fs::write(game_dir.join(wilds_chain().filename(i)), b"vanilla").unwrap();
    }

    // Three mods, three different formats: the exact mix that failed.
    let loader_zip = tmp.path().join("REFramework.zip");
    zip_with(&loader_zip, &[("dinput8.dll", b"LOADER")]);

    let armor_zip = tmp.path().join("Armor.zip");
    zip_with(
        &armor_zip,
        &[
            ("Armor/modinfo.ini", b"name=Armor\nversion=v1\n"),
            ("Armor/natives/stm/art/x.mesh.241111606", b"MESH"),
            ("Armor/reframework/data/x.json", b"{}"),
        ],
    );

    let pak_zip = tmp.path().join("Textures.zip");
    zip_with(
        &pak_zip,
        &[
            ("Tex/modinfo.ini", b"name=Textures\nversion=v1\n"),
            ("Tex/VerRBodyTextures-0-basic.pak", b"PAKDATA"),
        ],
    );

    // Analyze + stage each mod under its own namespace in the shared root.
    let mut parts: Vec<ModPlan> = Vec::new();
    for (mod_id, zip, priority) in [
        ("mod-loader", &loader_zip, 0i64),
        ("mod-armor", &armor_zip, 1),
        ("mod-textures", &pak_zip, 2),
    ] {
        let bundle = apoc_modengine::analyze_archive_with(zip, &wilds_rules()).unwrap();
        let staging = staging_root.join(mod_id);
        apoc_modengine::stage_bundle(zip, &bundle, &staging).unwrap();
        let sel = apoc_modengine::default_selection(&bundle);
        let plan = apoc_modengine::plan_deployment(&bundle, &sel);
        assert!(
            !plan.files.is_empty(),
            "{mod_id} produced no files: it would silently not install"
        );
        parts.push(ModPlan::same_namespace(mod_id, priority, plan));
    }

    let combined = apoc_modengine::combine_plans(parts, "3 mods");
    assert!(combined.is_valid(), "issues: {:?}", combined.issues);
    assert_eq!(combined.file_count(), 4, "loader + mesh + json + pak");

    let ctx = DeployContext {
        game_id: "monster-hunter-wilds".into(),
        game_dir: game_dir.clone(),
        staging_dir: staging_root.clone(),
        vault_dir: tmp.path().join("vault"),
        journal_dir: tmp.path().join("journal"),
        ladder: Ladder::default(),
        pak_chain: Some(wilds_chain()),
        // Wilds' proxy is a root-level DLL, which the fallback already covers.
        copy_only_paths: Vec::new(),
    };

    let dr = dry_run(&ctx, &combined).unwrap();
    assert!(
        dr.missing.is_empty(),
        "staged files missing: {:?}",
        dr.missing
    );

    let journal = apply(&ctx, &combined).unwrap();

    // Every format landed where the game expects it.
    assert_eq!(
        std::fs::read(game_dir.join("dinput8.dll")).unwrap(),
        b"LOADER",
        "REFramework must be installed or nothing else loads"
    );
    // Casing is normalised to what RE Engine resolves, so a Windows-authored
    // archive lands in one tree on a case-sensitive filesystem.
    assert!(game_dir.join("natives/STM/art/x.mesh.241111606").is_file());
    assert!(game_dir.join("reframework/data/x.json").is_file());
    assert!(
        game_dir.join(wilds_chain().filename(16)).is_file(),
        "the pak joins the chain above the vanilla patches"
    );

    // Vanilla patches are untouched.
    assert_eq!(
        std::fs::read(game_dir.join(wilds_chain().filename(15))).unwrap(),
        b"vanilla"
    );

    // One rollback removes everything the profile added.
    let report = rollback(&ctx, &journal, None);
    assert!(report.is_clean(), "{report:?}");
    assert!(!game_dir.join("dinput8.dll").exists());
    assert!(!game_dir.join("natives").exists());
    assert!(!game_dir.join("reframework").exists());
    assert!(!game_dir.join(wilds_chain().filename(16)).exists());
    assert_eq!(
        std::fs::read_dir(&game_dir).unwrap().count(),
        15,
        "only the vanilla patch chain remains"
    );

    eprintln!(
        "multi-mod deploy ok: {} files, rolled back clean",
        combined.file_count()
    );
}

/// A second apply must not double-count the pak chain if the first was reverted.
#[test]
fn pak_index_does_not_drift_across_reverted_deployments() {
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("game");
    let staging_root = tmp.path().join("staging");
    std::fs::create_dir_all(&game_dir).unwrap();

    let pak_zip = tmp.path().join("Tex.zip");
    zip_with(
        &pak_zip,
        &[
            ("Tex/modinfo.ini", b"name=Textures\n"),
            ("Tex/tex.pak", b"P"),
        ],
    );
    let bundle = apoc_modengine::analyze_archive_with(&pak_zip, &wilds_rules()).unwrap();
    apoc_modengine::stage_bundle(&pak_zip, &bundle, &staging_root.join("mod-tex")).unwrap();
    let plan = apoc_modengine::combine_plans(
        vec![ModPlan::same_namespace(
            "mod-tex",
            0,
            apoc_modengine::plan_deployment(&bundle, &apoc_modengine::default_selection(&bundle)),
        )],
        "1 mod",
    );

    let ctx = DeployContext {
        game_id: "g".into(),
        game_dir: game_dir.clone(),
        staging_dir: staging_root,
        vault_dir: tmp.path().join("vault"),
        journal_dir: tmp.path().join("journal"),
        ladder: Ladder::default(),
        pak_chain: Some(wilds_chain()),
        // Wilds' proxy is a root-level DLL, which the fallback already covers.
        copy_only_paths: Vec::new(),
    };

    let j1 = apply(&ctx, &plan).unwrap();
    assert!(game_dir.join(wilds_chain().filename(1)).is_file());
    rollback(&ctx, &j1, None);

    let _j2 = apply(&ctx, &plan).unwrap();
    assert!(
        game_dir.join(wilds_chain().filename(1)).is_file(),
        "index reuses the freed slot instead of climbing forever"
    );
}
