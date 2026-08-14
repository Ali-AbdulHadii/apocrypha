//! Loader archives: a bare proxy DLL at the archive root, with no payload
//! folders. REFramework's release zip is exactly this shape, and it is how a
//! Linux user actually gets a loader into a Proton prefix.

use apoc_domain::{DeployRoot, InstallerModel, SelectMode};
use apoc_modengine::GameRules;
use std::io::Write;

/// Build a stand-in for REFramework's release zip: `dinput8.dll` plus a
/// metadata text file, both at the root.
fn loader_zip(dir: &std::path::Path) -> std::path::PathBuf {
    let path = dir.join("REFramework.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip.start_file("dinput8.dll", opts).unwrap();
    zip.write_all(b"MZ fake loader payload").unwrap();
    zip.start_file("reframework_revision.txt", opts).unwrap();
    zip.write_all(b"revision 1234").unwrap();
    zip.finish().unwrap();
    path
}

fn mhw_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["natives".into(), "reframework".into()],
        root_files: vec![("dinput8.dll".into(), "dinput8.dll".into())],
        accepts_pak: true,
        rewrap: vec![],
        canonical_case: vec!["STM".into()],
        formats: Vec::new(),
        fomod_dest_prefix: String::new(),
    }
}

#[test]
fn recognizes_a_root_level_loader_dll() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = loader_zip(tmp.path());

    let bundle = apoc_modengine::analyze_archive_with(&zip, &mhw_rules()).expect("analyze");

    assert_eq!(bundle.installer_model, InstallerModel::Loader);
    assert_eq!(bundle.option_count(), 1);

    let opt = bundle.options().next().unwrap();
    assert!(opt.deployable, "the loader must be installable");
    assert_eq!(
        opt.select_mode,
        SelectMode::Forced,
        "a single-option loader is all-or-nothing, not an info card"
    );

    // Only the declared loader file deploys; the revision text is metadata.
    assert_eq!(opt.payload.len(), 1);
    let f = &opt.payload[0];
    assert_eq!(f.game_rel_path, "dinput8.dll");
    assert_eq!(f.root, DeployRoot::GameRoot);
}

#[test]
fn without_a_loader_rule_the_same_archive_is_unrecognized() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = loader_zip(tmp.path());

    // A game whose profile declares no loader must not silently install DLLs.
    let bundle = apoc_modengine::analyze_archive_with(&zip, &GameRules::default()).unwrap();
    assert_eq!(bundle.installer_model, InstallerModel::Unknown);
    assert_eq!(bundle.deployable_options().count(), 0);
}

#[test]
fn staging_and_planning_carry_the_loader_to_the_game_root() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = loader_zip(tmp.path());
    let bundle = apoc_modengine::analyze_archive_with(&zip, &mhw_rules()).unwrap();

    let staging = tmp.path().join("staging");
    let report = apoc_modengine::stage_bundle(&zip, &bundle, &staging).unwrap();
    assert_eq!(report.files_written, 1);

    let sel = apoc_modengine::default_selection(&bundle);
    let plan = apoc_modengine::plan_deployment(&bundle, &sel);
    assert!(plan.is_valid(), "issues: {:?}", plan.issues);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].game_rel_path, "dinput8.dll");
    assert!(staging.join(&plan.files[0].staged_rel_path).is_file());
}

/// A Fluffy single mod whose entire payload is one `.pak` beside `modinfo.ini`.
#[test]
fn recognizes_a_standalone_pak_mod() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("PakMod.zip");
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("Ver.R body Basic Textures v1.06/modinfo.ini", opts)
            .unwrap();
        zip.write_all(b"name=Ver.R ---Basic Textures---\nversion=v1.06\nscreenshot=00-basic.jpg\n")
            .unwrap();
        zip.start_file("Ver.R body Basic Textures v1.06/00-basic.jpg", opts)
            .unwrap();
        zip.write_all(b"\xff\xd8\xff").unwrap();
        zip.start_file(
            "Ver.R body Basic Textures v1.06/VerRBodyTextures-0-basic.pak",
            opts,
        )
        .unwrap();
        zip.write_all(b"PAK CONTENT").unwrap();
        zip.finish().unwrap();
    }

    let bundle = apoc_modengine::analyze_archive_with(&path, &mhw_rules()).unwrap();
    let opt = bundle.options().next().unwrap();

    assert!(opt.deployable, "a pak-only mod must be installable");
    assert_eq!(opt.select_mode, SelectMode::Forced);
    assert_eq!(opt.payload.len(), 1, "the jpg is a preview, not payload");
    assert_eq!(opt.payload[0].root, DeployRoot::Pak);
    assert_eq!(opt.payload[0].game_rel_path, "VerRBodyTextures-0-basic.pak");
    assert!(opt.screenshot_archive_path.is_some());

    // A game without a declared pak chain must not install it blindly.
    let plain = apoc_modengine::analyze_archive_with(&path, &GameRules::default()).unwrap();
    assert_eq!(plain.deployable_options().count(), 0);
}

/// The real archive, when present, must classify the same way.
#[test]
fn real_reframework_release_if_available() {
    let candidate = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join("Downloads/REFramework.zip");
    if !candidate.is_file() {
        eprintln!("SKIP: {} not present", candidate.display());
        return;
    }
    let bundle = apoc_modengine::analyze_archive_with(&candidate, &mhw_rules()).unwrap();
    assert_eq!(bundle.installer_model, InstallerModel::Loader);
    let opt = bundle.options().next().unwrap();
    assert!(opt.deployable);
    assert!(opt
        .payload
        .iter()
        .any(|f| f.game_rel_path.eq_ignore_ascii_case("dinput8.dll")));
    eprintln!(
        "real REFramework: {} option(s), {} file(s)",
        bundle.option_count(),
        opt.payload.len()
    );
}

/// The user's real PAK-only mod, when present.
#[test]
fn real_pak_mod_if_available() {
    let dl = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Downloads");
    let Some(found) = std::fs::read_dir(&dl).ok().and_then(|rd| {
        rd.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("Basic Textures") && n.ends_with(".zip"))
        })
    }) else {
        eprintln!("SKIP: no Basic Textures archive in ~/Downloads");
        return;
    };

    let bundle = apoc_modengine::analyze_archive_with(&found, &mhw_rules()).unwrap();
    let opt = bundle.options().next().expect("one option");
    assert!(opt.deployable, "the real pak mod must be installable");
    assert!(opt
        .payload
        .iter()
        .any(|f| f.root == DeployRoot::Pak && f.game_rel_path.ends_with(".pak")));
    eprintln!(
        "real pak mod '{}': {} payload file(s)",
        bundle.name,
        opt.payload.len()
    );
}
