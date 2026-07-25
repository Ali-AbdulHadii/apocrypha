//! Integration test against the REAL sample mod in the repository root:
//! `Ver.R Hirabami FM AIO ....zip`: a Fluffy segmented AIO for Monster Hunter
//! Wilds. This is the Phase-1 proof that the engine understands real-world
//! formats, not a synthetic fixture.
//!
//! If the archive is not present (e.g. CI without the asset), the test skips
//! with a message instead of failing.

use apoc_domain::{DeployRoot, InstallerModel, SelectMode};
use std::path::PathBuf;

/// Locate the sample AIO zip in the repo root (three levels up from this crate).
fn sample_zip() -> Option<PathBuf> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..");
    let dir = std::fs::read_dir(&repo_root).ok()?;
    for entry in dir.flatten() {
        let path = entry.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".zip") && name.contains("AIO") {
            return Some(path);
        }
    }
    None
}

#[test]
fn analyzes_real_fluffy_aio_sample() {
    let Some(zip) = sample_zip() else {
        eprintln!("SKIP: sample AIO zip not found in repo root; skipping real-mod test");
        return;
    };
    eprintln!("Analyzing sample: {}", zip.display());

    let bundle = apoc_modengine::analyze_archive(&zip).expect("sample must analyze");

    // 1. Recognized as a Fluffy segmented AIO.
    assert_eq!(bundle.installer_model, InstallerModel::FluffyAio);

    // 2. Collapsed to ONE logical bundle via NameAsBundle.
    assert_eq!(bundle.name, "Ver.R Hirabami F-M Armor");
    assert_eq!(bundle.version.as_deref(), Some("v1.03"));

    // 3. Many options across multiple part groups.
    assert!(
        bundle.option_count() >= 40,
        "expected ~50 options, got {}",
        bundle.option_count()
    );
    assert!(
        bundle.groups.len() >= 5,
        "expected several part groups, got {}",
        bundle.groups.len()
    );

    // 4. A forced "basic files (must install)" option exists and is deployable.
    let forced: Vec<_> = bundle
        .options()
        .filter(|o| o.select_mode == SelectMode::Forced)
        .collect();
    assert!(!forced.is_empty(), "must have a forced basic option");
    assert!(
        forced
            .iter()
            .any(|o| o.name.to_lowercase().contains("must install")),
        "forced option should be the 'must install' basics"
    );

    // 5. Mutually-exclusive variants exist ("select only one").
    let exclusive = bundle
        .options()
        .filter(|o| o.select_mode == SelectMode::Exclusive)
        .count();
    assert!(
        exclusive >= 5,
        "expected several exclusive variants, got {exclusive}"
    );

    // 6. Info-only notices (cover/warning) exist and are NOT deployable.
    let info: Vec<_> = bundle
        .options()
        .filter(|o| o.select_mode == SelectMode::Info)
        .collect();
    assert!(!info.is_empty(), "expected cover/warning info options");
    assert!(
        info.iter().all(|o| !o.deployable),
        "info options must never be deployable"
    );

    // 7. RE Engine payloads are preserved verbatim under natives/, with versioned
    //    extensions intact (e.g. `.mesh.241111606`).
    let has_versioned_mesh = bundle.deployable_options().any(|o| {
        o.payload.iter().any(|f| {
            f.root == DeployRoot::Natives
                && f.game_rel_path.starts_with("natives/")
                && f.game_rel_path.contains(".mesh.")
        })
    });
    assert!(has_versioned_mesh, "expected natives/*.mesh.<ver> payloads");

    // 8. REFramework data payloads are recognized under reframework/.
    let has_reframework = bundle
        .deployable_options()
        .any(|o| o.payload.iter().any(|f| f.root == DeployRoot::Reframework));
    assert!(has_reframework, "expected reframework/ payloads");

    // 9. Archive hash was computed.
    assert!(bundle
        .archive_sha256
        .as_ref()
        .is_some_and(|h| h.len() == 64));

    // 10. `archive_path` must stay resolvable inside the real zip even though
    //     the wrapper directory is stripped from the game-relative path.
    let names: std::collections::HashSet<String> = {
        let f = std::fs::File::open(&zip).unwrap();
        let mut z = zip::ZipArchive::new(std::io::BufReader::new(f)).unwrap();
        (0..z.len())
            .filter_map(|i| z.by_index(i).ok().map(|e| e.name().replace('\\', "/")))
            .collect()
    };
    for opt in bundle.deployable_options() {
        for file in &opt.payload {
            assert!(
                names.contains(&file.archive_path),
                "archive_path '{}' is not a real entry in the zip",
                file.archive_path
            );
            assert!(
                !file.game_rel_path.starts_with("Ver.R"),
                "game_rel_path must have the wrapper folder stripped: {}",
                file.game_rel_path
            );
        }
    }

    // 11. Preview images referenced by `screenshot=` resolve to real entries,
    //     including the info-only cover/warning cards that carry no payload.
    let with_screenshot: Vec<_> = bundle
        .options()
        .filter(|o| o.screenshot.is_some())
        .collect();
    assert!(
        with_screenshot.len() >= 20,
        "expected many options to declare a screenshot, got {}",
        with_screenshot.len()
    );
    let resolved = with_screenshot
        .iter()
        .filter(|o| o.screenshot_archive_path.is_some())
        .count();
    assert_eq!(
        resolved,
        with_screenshot.len(),
        "every declared screenshot should resolve to a real archive entry"
    );
    for o in &with_screenshot {
        let p = o.screenshot_archive_path.as_ref().unwrap();
        assert!(names.contains(p), "screenshot path '{p}' not in the zip");
    }
    assert!(
        info.iter().any(|o| o.screenshot_archive_path.is_some()),
        "cover/warning notices must carry preview images"
    );

    // 12. Staging extracts those previews alongside the payload.
    let tmp = tempfile::tempdir().unwrap();
    let report = apoc_modengine::stage_bundle(&zip, &bundle, tmp.path()).unwrap();
    assert!(
        report.previews_written >= 20,
        "expected previews to be staged, got {}",
        report.previews_written
    );
    let cover = info
        .iter()
        .find(|o| o.screenshot.is_some())
        .expect("an info option with a screenshot");
    assert!(
        apoc_modengine::staged_preview(tmp.path(), &cover.id, cover.screenshot.as_ref().unwrap())
            .is_some(),
        "cover preview should be readable from the staging dir"
    );

    // Human-readable summary for `cargo test -- --nocapture`.
    eprintln!(
        "bundle='{}' v{} model={:?} groups={} options={} (forced={}, exclusive={}, info={})",
        bundle.name,
        bundle.version.as_deref().unwrap_or("?"),
        bundle.installer_model,
        bundle.groups.len(),
        bundle.option_count(),
        forced.len(),
        exclusive,
        info.len()
    );
}
