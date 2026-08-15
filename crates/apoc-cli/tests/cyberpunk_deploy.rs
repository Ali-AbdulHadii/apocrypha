//! Installing a Cyberpunk profile over a game that already has files in it.
//!
//! Cyberpunk has never had a deploy test. That matters more here than it did for
//! Wilds, because a Cyberpunk mod overwrites a file the game ships: redscript
//! replaces `engine/config/base/scripts.ini`, which exists in a clean install.
//! Every Wilds mod only ever adds files, so vault-before-overwrite — the first
//! of the three safety invariants — has never run for this game in any test.
//!
//! Synthesized rather than driven from the real archives, so it runs in CI where
//! the fixtures do not exist. The shapes are taken from the five real releases;
//! `cyberpunk_real_archives.rs` is what proves they are the real shapes.

use apoc_deploy::journal::JournalOp;
use apoc_deploy::{apply, dry_run, place::Ladder, rollback, DeployContext};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_modengine::{GameRules, ModPlan};
use std::io::Write;
use std::path::Path;

/// The one file in this test that the game itself ships.
const VANILLA_INI: &str = "engine/config/base/scripts.ini";
const VANILLA_INI_BODY: &[u8] = b"[Scripts]\nEnableCompilation = false\n";

fn cp77_profile() -> apoc_domain::GameProfile {
    LocalBuiltin::new()
        .get("cyberpunk-2077")
        .expect("the cyberpunk profile ships")
}

/// Rules from the shipped profile, never written here, so a profile change that
/// would break a real deploy fails in this file.
fn cp77_rules() -> GameRules {
    GameRules::from_profile(&cp77_profile())
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

/// A clean install: the binary, one shipped content archive, and the config file
/// redscript is about to replace.
fn seed_game_dir(game_dir: &Path) {
    std::fs::create_dir_all(game_dir.join("bin/x64")).unwrap();
    std::fs::write(game_dir.join("bin/x64/Cyberpunk2077.exe"), b"GAME").unwrap();

    std::fs::create_dir_all(game_dir.join("archive/pc/content")).unwrap();
    std::fs::write(
        game_dir.join("archive/pc/content/basegame_1_engine.archive"),
        b"SHIPPED CONTENT",
    )
    .unwrap();

    std::fs::create_dir_all(game_dir.join("engine/config/base")).unwrap();
    std::fs::write(game_dir.join(VANILLA_INI), VANILLA_INI_BODY).unwrap();
}

/// The four mods, in the shapes the real releases use. `mod-archive` is
/// deliberately mis-cased: a Windows-authored archive shipping `Archive/`.
fn build_mods(tmp: &Path) -> Vec<(&'static str, std::path::PathBuf)> {
    let red4ext = tmp.join("RED4ext.zip");
    zip_with(
        &red4ext,
        &[
            ("bin/x64/winmm.dll", b"RED4EXT PROXY"),
            ("red4ext/RED4ext.dll", b"RED4EXT"),
            ("red4ext/LICENSE.txt", b"MIT"),
        ],
    );

    let cet = tmp.join("CET.zip");
    zip_with(
        &cet,
        &[
            ("bin/x64/version.dll", b"CET PROXY"),
            ("bin/x64/plugins/cyber_engine_tweaks.asi", b"CET ASI"),
            (
                "bin/x64/plugins/cyber_engine_tweaks/tweakdb/usedhashes.kark",
                b"HASHES",
            ),
        ],
    );

    // The mod that overwrites a shipped file.
    let redscript = tmp.join("redscript.zip");
    zip_with(
        &redscript,
        &[
            (
                VANILLA_INI,
                b"[Scripts]\nEnableCompilation = true\nScriptsBlacklist = \n",
            ),
            ("engine/tools/scc.exe", b"COMPILER"),
            ("r6/config/cybercmd/scc.toml", b"[[tasks]]"),
        ],
    );

    // Mis-cased on purpose: `Archive/PC/Mod/` must fold into the tree the game
    // reads, not create a second one beside it.
    let asset = tmp.join("VanillaRefit.zip");
    zip_with(
        &asset,
        &[("Archive/PC/Mod/VanillaRefit.archive", b"TEXTURES")],
    );

    vec![
        ("mod-red4ext", red4ext),
        ("mod-cet", cet),
        ("mod-redscript", redscript),
        ("mod-asset", asset),
    ]
}

/// Analyse, stage and plan every mod into one combined transaction.
fn combined_plan(tmp: &Path, staging_root: &Path) -> apoc_domain::DeploymentPlan {
    let rules = cp77_rules();
    let mut parts: Vec<ModPlan> = Vec::new();
    for (i, (mod_id, zip)) in build_mods(tmp).into_iter().enumerate() {
        let bundle = apoc_modengine::analyze_archive_with(&zip, &rules).unwrap();
        let staging = staging_root.join(mod_id);
        apoc_modengine::stage_bundle(&zip, &bundle, &staging).unwrap();
        let sel = apoc_modengine::default_selection(&bundle);
        let plan = apoc_modengine::plan_deployment(&bundle, &sel);
        assert!(
            !plan.files.is_empty(),
            "{mod_id} produced no files: it would silently not install"
        );
        parts.push(ModPlan::same_namespace(mod_id, i as i64, plan));
    }
    apoc_modengine::combine_plans(parts, "4 mods")
}

fn context(tmp: &Path, game_dir: &Path, staging_root: &Path) -> DeployContext {
    DeployContext {
        game_id: "cyberpunk-2077".into(),
        game_dir: game_dir.to_path_buf(),
        staging_dir: staging_root.to_path_buf(),
        vault_dir: tmp.join("vault"),
        journal_dir: tmp.join("journal"),
        ladder: Ladder::default(),
        // Cyberpunk declares no patch chain: REDengine loads every `.archive`
        // under `archive/pc/mod` directly, so nothing may be renamed.
        pak_chain: None,
        copy_only_paths: DeployContext::copy_only_from(&cp77_profile()),
    }
}

#[test]
fn a_shipped_config_file_is_vaulted_before_it_is_replaced_and_restored_after() {
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("Cyberpunk 2077");
    let staging_root = tmp.path().join("staging");
    seed_game_dir(&game_dir);

    let plan = combined_plan(tmp.path(), &staging_root);
    assert!(plan.is_valid(), "issues: {:?}", plan.issues);
    assert_eq!(plan.file_count(), 10, "3 + 3 + 3 + 1 files");

    let ctx = context(tmp.path(), &game_dir, &staging_root);

    // ---- Dry run says what will be replaced, and changes nothing -----------
    let dr = dry_run(&ctx, &plan).unwrap();
    assert!(
        dr.missing.is_empty(),
        "staged files missing: {:?}",
        dr.missing
    );
    assert_eq!(
        dr.replaces,
        vec![VANILLA_INI.to_string()],
        "exactly one shipped file is at risk, and the user is told which"
    );
    assert_eq!(
        std::fs::read(game_dir.join(VANILLA_INI)).unwrap(),
        VANILLA_INI_BODY,
        "a dry run must not touch the game"
    );
    assert!(
        !game_dir.join("red4ext").exists(),
        "a dry run must not deploy"
    );

    // ---- Apply ------------------------------------------------------------
    let journal = apply(&ctx, &plan).unwrap();

    // Both proxies coexist in one folder. This is what the profile's
    // `winmm=n,b;version=n,b` describes, and nothing had exercised it.
    assert_eq!(
        std::fs::read(game_dir.join("bin/x64/winmm.dll")).unwrap(),
        b"RED4EXT PROXY"
    );
    assert_eq!(
        std::fs::read(game_dir.join("bin/x64/version.dll")).unwrap(),
        b"CET PROXY"
    );

    // The overwrite happened.
    assert_ne!(
        std::fs::read(game_dir.join(VANILLA_INI)).unwrap(),
        VANILLA_INI_BODY,
        "redscript must actually replace the shipped config"
    );

    // The mis-cased mod folded into the existing tree rather than making a new
    // one. On a case-insensitive filesystem this passes trivially, which is
    // exactly why the child count is asserted rather than the path's existence.
    assert!(game_dir
        .join("archive/pc/mod/VanillaRefit.archive")
        .is_file());
    let archive_children: Vec<String> = std::fs::read_dir(game_dir.join("archive"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        archive_children,
        vec!["pc".to_string()],
        "the mis-cased mod created a second tree the game never reads"
    );

    // The vault ran. Asserted from the journal rather than inferred from a
    // successful rollback: an empty vault key with a rollback that happens to
    // work is the failure this is looking for.
    let replaced: Vec<&JournalOp> = journal
        .ops()
        .iter()
        .filter(|op| matches!(op, JournalOp::Replaced { .. }))
        .collect();
    assert_eq!(replaced.len(), 1, "exactly one shipped file was replaced");
    match replaced[0] {
        JournalOp::Replaced {
            game_rel_path,
            original_vault_key,
            ..
        } => {
            assert_eq!(game_rel_path, VANILLA_INI);
            assert!(
                !original_vault_key.is_empty(),
                "the original was replaced without being vaulted"
            );
        }
        _ => unreachable!(),
    }

    // Files the game owns and no mod claims are untouched.
    assert_eq!(
        std::fs::read(game_dir.join("bin/x64/Cyberpunk2077.exe")).unwrap(),
        b"GAME"
    );
    assert_eq!(
        std::fs::read(game_dir.join("archive/pc/content/basegame_1_engine.archive")).unwrap(),
        b"SHIPPED CONTENT"
    );

    // ---- Roll back --------------------------------------------------------
    let report = rollback(&ctx, &journal, None);
    assert!(report.is_clean(), "rollback not clean: {report:?}");

    assert_eq!(
        std::fs::read(game_dir.join(VANILLA_INI)).unwrap(),
        VANILLA_INI_BODY,
        "the shipped config must come back byte-identical"
    );
    assert!(!game_dir.join("red4ext").exists());
    assert!(!game_dir.join("r6").exists());
    assert!(!game_dir.join("bin/x64/winmm.dll").exists());
    assert!(!game_dir.join("bin/x64/version.dll").exists());
    assert!(!game_dir
        .join("archive/pc/mod/VanillaRefit.archive")
        .exists());

    // Directories the game owns survive even though a mod wrote inside them.
    assert!(game_dir.join("bin/x64/Cyberpunk2077.exe").is_file());
    assert!(game_dir
        .join("archive/pc/content/basegame_1_engine.archive")
        .is_file());
    assert!(
        game_dir.join("engine/config/base").is_dir(),
        "removing a directory the game ships would break the install"
    );
}

#[test]
fn the_vault_still_has_the_original_on_a_second_install() {
    // The failure a user meets on their second install rather than their first:
    // a vault entry consumed by the first rollback instead of copied out of.
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("Cyberpunk 2077");
    let staging_root = tmp.path().join("staging");
    seed_game_dir(&game_dir);

    let plan = combined_plan(tmp.path(), &staging_root);
    let ctx = context(tmp.path(), &game_dir, &staging_root);

    for cycle in 1..=2 {
        let journal = apply(&ctx, &plan).unwrap();
        assert_ne!(
            std::fs::read(game_dir.join(VANILLA_INI)).unwrap(),
            VANILLA_INI_BODY,
            "cycle {cycle}: the config was not replaced"
        );

        let report = rollback(&ctx, &journal, None);
        assert!(report.is_clean(), "cycle {cycle}: {report:?}");
        assert_eq!(
            std::fs::read(game_dir.join(VANILLA_INI)).unwrap(),
            VANILLA_INI_BODY,
            "cycle {cycle}: the vaulted original did not come back"
        );
    }
}

#[test]
fn a_proxy_in_a_subdirectory_is_still_a_real_copy() {
    // `place.rs` says the loader DLL is "always a real copy, never linked", and
    // `apoc game cyberpunk-2077` prints the same promise. The test for it asked
    // whether the path had a `/` in it, which is true of REFramework's
    // root-level `dinput8.dll` and false of both of Cyberpunk's, so both were
    // being hardlinked. Wine resolves the proxy before the game starts and link
    // indirection there is a known cause of a loader silently not loading.
    let tmp = tempfile::tempdir().unwrap();
    let game_dir = tmp.path().join("Cyberpunk 2077");
    let staging_root = tmp.path().join("staging");
    seed_game_dir(&game_dir);

    let plan = combined_plan(tmp.path(), &staging_root);
    let ctx = context(tmp.path(), &game_dir, &staging_root);
    let journal = apply(&ctx, &plan).unwrap();

    // Read from the journal, which records what was actually done, rather than
    // from the filesystem, where a hardlink and a copy look the same.
    for proxy in ["bin/x64/winmm.dll", "bin/x64/version.dll"] {
        let method = journal
            .ops()
            .iter()
            .find_map(|op| match op {
                JournalOp::Created {
                    game_rel_path,
                    method,
                    ..
                } if game_rel_path == proxy => Some(*method),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{proxy} was never deployed"));
        assert_eq!(
            method,
            apoc_domain::DeployMethod::Copy,
            "{proxy} was linked, against a guarantee the tool prints"
        );
    }

    // A file that is not a proxy still takes the context's ladder, so this is
    // not simply copy-everything with extra steps.
    let other = journal
        .ops()
        .iter()
        .find_map(|op| match op {
            JournalOp::Created {
                game_rel_path,
                method,
                ..
            } if game_rel_path == "red4ext/RED4ext.dll" => Some(*method),
            _ => None,
        })
        .expect("the framework DLL was deployed");
    assert_ne!(
        other,
        apoc_domain::DeployMethod::Copy,
        "only proxies are copied; everything else uses the link ladder"
    );
}

#[test]
fn both_of_cyberpunks_proxies_are_registered_in_the_prefix() {
    // Cyberpunk is the first game with two independent proxies in one folder,
    // and registering only the first leaves half the stack silently dead. The
    // pairs are read from the profile so a TOML edit fails here.
    let profile = cp77_profile();
    let loader = profile
        .loader
        .as_ref()
        .expect("cyberpunk declares a loader");
    let overrides = loader.dll_overrides();
    assert_eq!(
        overrides.len(),
        2,
        "RED4ext takes winmm, Cyber Engine Tweaks takes version: {overrides:?}"
    );

    let tmp = tempfile::tempdir().unwrap();
    let reg = tmp.path().join("pfx/user.reg");
    std::fs::create_dir_all(reg.parent().unwrap()).unwrap();
    std::fs::write(&reg, "WINE REGISTRY Version 2\n\n[Software\\\\Wine]\n").unwrap();

    // A pre-existing override the user set themselves, which must survive.
    apoc_deploy::loader::write_override(&reg, "dsound", "native").unwrap();

    let mut previous = Vec::new();
    for (name, value) in &overrides {
        previous.push((
            name.clone(),
            apoc_deploy::loader::write_override(&reg, name, value).unwrap(),
        ));
    }

    // Both read back. Writing the second must not have displaced the first.
    for (name, value) in &overrides {
        assert_eq!(
            apoc_deploy::loader::read_override(&reg, name)
                .unwrap()
                .value
                .as_deref(),
            Some(value.as_str()),
            "{name} is not registered, so half the stack would not load"
        );
    }

    for (name, prev) in &previous {
        apoc_deploy::loader::restore_override(&reg, name, prev.as_deref()).unwrap();
        assert_eq!(
            apoc_deploy::loader::read_override(&reg, name)
                .unwrap()
                .value,
            None,
            "{name} outlived the deploy that registered it"
        );
    }
    assert_eq!(
        apoc_deploy::loader::read_override(&reg, "dsound")
            .unwrap()
            .value
            .as_deref(),
        Some("native"),
        "an override the user set themselves must survive our rollback"
    );
}
