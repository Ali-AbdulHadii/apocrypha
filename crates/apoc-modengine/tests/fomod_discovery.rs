//! Finding a FOMOD manifest, and deciding that an archive is one.
//!
//! Discovery is where the format's two sharp edges live. Casing is one: the
//! directory and the file both appear in every combination, because the tools
//! that write them are Windows tools and Windows does not care. The other is
//! precedence, since a repack can carry a manifest *and* the loose metadata of
//! another format, and only one of those was written on purpose.
//!
//! Archives are built here rather than checked in, following the rest of this
//! directory: a fixture cannot then drift out of sync with what the engine
//! claims to support.

use apoc_modengine::{ArchiveIndex, GameRules};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A profile that expects FOMOD, as a Bethesda game's would.
fn fomod_rules() -> GameRules {
    GameRules {
        formats: vec!["fomod".into(), "loose-roots".into()],
        ..GameRules::default()
    }
}

/// A profile that does not.
fn no_fomod_rules() -> GameRules {
    GameRules {
        formats: vec!["fluffy-aio".into()],
        ..GameRules::default()
    }
}

const MINIMAL_CONFIG: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<config><moduleName>Test Mod</moduleName></config>"#;

fn zip_with(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("mod.zip");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

fn index_of(entries: &[(&str, &[u8])]) -> (ArchiveIndex, PathBuf, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let path = zip_with(tmp.path(), entries);
    let index = apoc_modengine::read_index(&path).unwrap();
    (index, path, tmp)
}

#[test]
fn a_manifest_is_found_however_its_name_is_cased() {
    for (dir, file) in [
        ("fomod", "ModuleConfig.xml"),
        ("FOMOD", "MODULECONFIG.XML"),
        ("Fomod", "moduleconfig.xml"),
    ] {
        let (index, _path, _tmp) = index_of(&[
            (&format!("{dir}/{file}"), MINIMAL_CONFIG),
            ("meshes/x.nif", b"N"),
        ]);
        let found = index
            .fomod
            .unwrap_or_else(|| panic!("{dir}/{file} should have been found"));
        assert_eq!(found.config, MINIMAL_CONFIG);
    }
}

#[test]
fn the_manifests_true_casing_is_kept_so_it_can_be_read_again() {
    // Discovery ignores case; extraction does not. If the path were normalised
    // on the way in, nothing would ever open the file a second time.
    let (index, path, _tmp) = index_of(&[("FOMOD/ModuleConfig.XML", MINIMAL_CONFIG)]);
    let found = index.fomod.expect("manifest found");

    assert_eq!(found.config_archive_path, "FOMOD/ModuleConfig.XML");
    let bytes = apoc_modengine::read_archive_entry(&path, &found.config_archive_path)
        .expect("the recorded path opens the entry");
    assert_eq!(bytes, MINIMAL_CONFIG);
}

#[test]
fn an_info_document_beside_the_manifest_is_captured_too() {
    let (index, _path, _tmp) = index_of(&[
        ("fomod/ModuleConfig.xml", MINIMAL_CONFIG),
        ("fomod/info.xml", b"<fomod><Author>A</Author></fomod>"),
    ]);
    let found = index.fomod.expect("manifest found");
    assert!(found.info.is_some());
}

#[test]
fn an_info_document_without_a_manifest_is_not_a_fomod() {
    // info.xml is metadata. The manifest is what declares an installer, and
    // without one there is nothing to install from.
    let (index, _path, _tmp) = index_of(&[("fomod/info.xml", b"<fomod/>"), ("meshes/x.nif", b"N")]);
    assert!(index.fomod.is_none());
}

#[test]
fn a_stray_info_xml_elsewhere_is_somebody_s_documentation() {
    let (index, _path, _tmp) = index_of(&[
        ("docs/info.xml", b"<x/>"),
        ("readme/moduleconfig.xml", b"<x/>"),
    ]);
    assert!(
        index.fomod.is_none(),
        "neither file sits in a directory named fomod"
    );
}

#[test]
fn the_outermost_manifest_wins_when_a_repack_contains_two() {
    let (index, _path, _tmp) = index_of(&[
        ("fomod/ModuleConfig.xml", b"<config>outer</config>"),
        (
            "Extras/Second Mod/fomod/ModuleConfig.xml",
            b"<config>inner</config>",
        ),
    ]);
    let found = index.fomod.expect("manifest found");
    assert_eq!(
        found.config, b"<config>outer</config>",
        "merging two installers would invent a third"
    );
}

/* ------------------------------------------------------------ detection --- */

/// A manifest that installs something, so detection can be observed through the
/// bundle it produces. One that declares no files at all is reported as an empty
/// archive whatever format it is written in, which says nothing about detection.
const INSTALLING_CONFIG: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<config>
  <moduleName>Test Mod</moduleName>
  <requiredInstallFiles><file source="core.esp"/></requiredInstallFiles>
</config>"#;

fn detected_as_fomod(rules: &GameRules, entries: &[(&str, &[u8])]) -> bool {
    let tmp = tempfile::tempdir().unwrap();
    let path = zip_with(tmp.path(), entries);
    apoc_modengine::analyze_archive_with(&path, rules)
        .map(|b| b.installer_model == apoc_domain::InstallerModel::Fomod)
        .unwrap_or(false)
}

#[test]
fn a_game_that_expects_fomod_follows_the_manifest() {
    assert!(detected_as_fomod(
        &fomod_rules(),
        &[
            ("fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("core.esp", b"E"),
        ]
    ));
}

#[test]
fn a_manifest_outranks_the_loose_metadata_of_another_format() {
    // A repack carrying both. The modinfo.ini files would otherwise make this a
    // Fluffy AIO, and every option would be installed at once.
    assert!(detected_as_fomod(
        &fomod_rules(),
        &[
            ("fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("core.esp", b"E"),
            ("Option A/modinfo.ini", b"name=A\n"),
            ("Option B/modinfo.ini", b"name=B\n"),
        ]
    ));
}

#[test]
fn a_game_that_does_not_expect_fomod_ignores_the_manifest() {
    // Not a refusal and not a FOMOD: it falls through to the chain that was
    // there before, which is exactly today's behaviour for such an archive.
    assert!(!detected_as_fomod(
        &no_fomod_rules(),
        &[
            ("fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("core.esp", b"E"),
            ("Option A/modinfo.ini", b"name=A\n"),
        ]
    ));
}

#[test]
fn an_archive_with_no_manifest_is_unaffected() {
    assert!(!detected_as_fomod(
        &fomod_rules(),
        &[("natives/stm/x.mesh", b"M")]
    ));
}

/* -------------------------------------------------------- wrapper strip --- */

#[test]
fn an_archive_that_is_only_a_fomod_directory_survives_wrapper_stripping() {
    // `fomod/` is the installer, not a wrapper around one. Stripping it would
    // leave an archive with nothing in it at all.
    assert!(detected_as_fomod(
        &fomod_rules(),
        &[
            ("fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("fomod/info.xml", b"<fomod/>"),
            ("core.esp", b"E"),
        ]
    ));
}

#[test]
fn a_manifest_inside_a_release_folder_is_still_found() {
    assert!(detected_as_fomod(
        &fomod_rules(),
        &[
            ("My Mod v1.0/fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("My Mod v1.0/core.esp", b"E"),
        ]
    ));
}

#[test]
fn cyberpunk_accepts_a_fomod_because_its_authors_ship_them() {
    // `formats` is a gate, not documentation: a non-empty list that omitted
    // `fomod` refused one outright. Cyberpunk's did, and Cyberpunk mods with
    // body-type or texture-resolution variants are shipped as FOMODs — the same
    // problem the format was written for.
    //
    // Read from the shipped profile rather than restated here, so removing the
    // entry fails in this test.
    use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
    let profile = LocalBuiltin::new().get("cyberpunk-2077").unwrap();
    let rules = GameRules::from_profile(&profile);

    assert!(rules.supports_format("fomod"));
    assert!(detected_as_fomod(
        &rules,
        &[
            ("fomod/ModuleConfig.xml", INSTALLING_CONFIG),
            ("archive/pc/mod/Body.archive", b"DATA"),
        ]
    ));

    // The formats it already had are untouched.
    assert!(rules.supports_format("loose-roots"));
    assert!(rules.supports_format("loader"));
    assert!(
        !rules.supports_format("fluffy-aio"),
        "the list is still a gate, not a free pass"
    );
}
