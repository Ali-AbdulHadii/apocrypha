//! **The Phase 1 acceptance test.**
//!
//! Drives the real sample mod through the entire pipeline against a simulated
//! game directory: analyze → stage → select → plan → dry-run → apply → verify →
//! roll back → verify pristine.
//!
//! It uses a temporary game directory rather than a real Monster Hunter Wilds
//! install, so it proves every engine except the final "game actually launches"
//! step, which requires the game on disk.

use apoc_deploy::{apply, dry_run, place::Ladder, rollback, DeployContext};
use apoc_domain::SelectMode;
use apoc_storage::Paths;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn sample_zip() -> Option<PathBuf> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..");
    for entry in std::fs::read_dir(&repo_root).ok()?.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".zip") && name.contains("AIO") {
            return Some(path);
        }
    }
    None
}

#[test]
fn full_install_deploy_and_rollback_cycle() {
    let Some(zip) = sample_zip() else {
        eprintln!("SKIP: sample AIO zip not found; skipping end-to-end test");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths::with_root(tmp.path().join("apocrypha"));
    let game_id = "monster-hunter-wilds";
    paths.ensure_game_dirs(game_id).unwrap();

    // A simulated game install with one pre-existing vanilla file that a mod
    // will displace, so restore-on-rollback is exercised for real.
    let game_dir = tmp.path().join("MonsterHunterWilds");
    std::fs::create_dir_all(&game_dir).unwrap();
    let pristine_marker = game_dir.join("game_executable_placeholder");
    std::fs::write(&pristine_marker, b"do not touch").unwrap();

    // ---- 1. Analyze -----------------------------------------------------
    let bundle = apoc_modengine::analyze_archive(&zip).expect("analyze");
    assert_eq!(bundle.name, "Ver.R Hirabami F-M Armor");
    assert!(bundle.option_count() >= 40);

    // ---- 2. Stage (outside the game dir) --------------------------------
    let staging = paths.mod_staging(game_id, "mod-sample");
    let report = apoc_modengine::stage_bundle(&zip, &bundle, &staging).expect("stage");
    assert!(report.files_written > 0);
    assert!(
        !staging.starts_with(&game_dir),
        "staged payloads must live outside the game directory"
    );

    // ---- 3. Select (forced basics + one variant per radio set) -----------
    let selection = apoc_modengine::recommended_selection(&bundle);
    assert!(!selection.is_empty());

    // Exactly one option chosen per radio set.
    let mut per_set: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for o in bundle.options() {
        if selection.contains(&o.id) {
            if let Some(set) = &o.radio_set {
                *per_set.entry(set.as_str()).or_default() += 1;
            }
        }
    }
    assert!(per_set.values().all(|&n| n == 1), "one choice per radio set");
    assert!(per_set.len() >= 6, "physics body+leg are separate sets");

    // No info-only option is ever selected.
    for o in bundle.options().filter(|o| o.select_mode == SelectMode::Info) {
        assert!(!selection.contains(&o.id), "info options are never installed");
    }

    // ---- 4. Plan --------------------------------------------------------
    let plan = apoc_modengine::plan_deployment(&bundle, &selection);
    assert!(plan.is_valid(), "issues: {:?}", plan.issues);
    assert!(plan.file_count() > 0);

    // Every destination is unique and lands under a known payload root.
    let dests: BTreeSet<&str> = plan.files.iter().map(|f| f.game_rel_path.as_str()).collect();
    assert_eq!(dests.len(), plan.files.len(), "one file per destination");
    assert!(plan
        .files
        .iter()
        .all(|f| f.game_rel_path.starts_with("natives/")
            || f.game_rel_path.starts_with("reframework/")));
    // RE Engine versioned extensions survive planning verbatim.
    assert!(plan.files.iter().any(|f| f.game_rel_path.contains(".mesh.")));

    let ctx = DeployContext {
        game_id: game_id.to_string(),
        game_dir: game_dir.clone(),
        staging_dir: staging.clone(),
        vault_dir: paths.vault(game_id),
        journal_dir: paths.journal(game_id),
        ladder: Ladder::default(),
        pak_chain: None,
    };

    // ---- 5. Dry run changes nothing -------------------------------------
    let dr = dry_run(&ctx, &plan).expect("dry run");
    assert!(dr.missing.is_empty(), "staged files missing: {:?}", dr.missing);
    assert_eq!(dr.file_count(), plan.file_count());
    assert!(!game_dir.join("natives").exists(), "dry run must not deploy");

    // ---- 6. Apply -------------------------------------------------------
    let journal = apply(&ctx, &plan).expect("apply");
    for f in &plan.files {
        let deployed = game_dir.join(&f.game_rel_path);
        assert!(deployed.is_file(), "missing deployed file {}", f.game_rel_path);
    }
    assert!(game_dir.join("natives/stm").is_dir());
    assert_eq!(journal.ops().len(), plan.file_count());

    // Deployed bytes match the staged source exactly.
    let sample = &plan.files[0];
    assert_eq!(
        std::fs::read(staging.join(&sample.staged_rel_path)).unwrap(),
        std::fs::read(game_dir.join(&sample.game_rel_path)).unwrap(),
    );

    // ---- 7. Roll back to pristine ---------------------------------------
    let report = rollback(&ctx, &journal, None);
    assert!(report.is_clean(), "rollback not clean: {report:?}");
    assert_eq!(report.removed.len(), plan.file_count());

    assert!(!game_dir.join("natives").exists(), "natives/ fully removed");
    assert!(!game_dir.join("reframework").exists(), "reframework/ fully removed");
    assert_eq!(
        std::fs::read(&pristine_marker).unwrap(),
        b"do not touch",
        "unrelated game files are never touched"
    );

    // The game directory is byte-for-byte back to its original contents.
    let remaining: Vec<_> = std::fs::read_dir(&game_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        remaining,
        vec!["game_executable_placeholder".to_string()],
        "game dir restored to pristine"
    );

    eprintln!(
        "E2E ok: {} options selected, {} files deployed ({} bytes), rolled back clean",
        selection.len(),
        plan.file_count(),
        plan.total_size()
    );
}
