//! Installing from a FOMOD: what the options are, and where the files go.
//!
//! Archives are synthesized here rather than checked in, following the rest of
//! this directory, so a fixture cannot drift from what the engine claims to
//! support. The manifests are written the way real ones are: mixed casing,
//! backslash separators, and destinations declared relative to the game.

use apoc_domain::{DeployRoot, ModBundle, SelectMode};
use apoc_modengine::{GameRules, ModEngineError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A game whose mods ship FOMODs and whose destinations are rooted at `Data`,
/// as a Bethesda title's are.
fn bethesda_rules() -> GameRules {
    GameRules {
        formats: vec!["fomod".into()],
        fomod_dest_prefix: "Data".into(),
        canonical_case: vec!["Data".into()],
        payload_roots: vec!["Data".into()],
        ..GameRules::default()
    }
}

/// A game that expects FOMODs but roots them at the game directory.
fn plain_rules() -> GameRules {
    GameRules {
        formats: vec!["fomod".into()],
        ..GameRules::default()
    }
}

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

fn analyze_with(
    rules: &GameRules,
    entries: &[(&str, &[u8])],
) -> (
    apoc_modengine::Result<ModBundle>,
    PathBuf,
    tempfile::TempDir,
) {
    let tmp = tempfile::tempdir().unwrap();
    let path = zip_with(tmp.path(), entries);
    let bundle = apoc_modengine::analyze_archive_with(&path, rules);
    (bundle, path, tmp)
}

fn analyze(
    rules: &GameRules,
    entries: &[(&str, &[u8])],
) -> (ModBundle, PathBuf, tempfile::TempDir) {
    let (bundle, path, tmp) = analyze_with(rules, entries);
    (bundle.expect("installs"), path, tmp)
}

fn dests(bundle: &ModBundle) -> Vec<String> {
    bundle
        .options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect()
}

const TWO_BODIES: &[u8] = br#"<?xml version="1.0" encoding="utf-8"?>
<config>
  <moduleName>Body Replacer</moduleName>
  <requiredInstallFiles>
    <file source="core\core.esp" destination="core.esp"/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Body">
      <optionalFileGroups>
        <group name="Shape" type="SelectExactlyOne">
          <plugins>
            <plugin name="Slim">
              <description>A slimmer shape.</description>
              <files><folder source="slim\meshes" destination="meshes"/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Muscular">
              <files><folder source="muscular\meshes" destination="meshes"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
        <group name="Extras" type="SelectAny">
          <plugins>
            <plugin name="High res textures">
              <files><folder source="textures" destination="textures"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

fn two_bodies_archive() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("fomod/ModuleConfig.xml", TWO_BODIES),
        ("core/core.esp", b"E" as &[u8]),
        ("slim/meshes/body.nif", b"S"),
        ("muscular/meshes/body.nif", b"M"),
        ("textures/body.dds", b"T"),
    ]
}

#[test]
fn the_authors_own_structure_becomes_the_wizards_structure() {
    let (bundle, _p, _t) = analyze(&bethesda_rules(), &two_bodies_archive());

    let labels: Vec<&str> = bundle.groups.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["Required files", "Body · Shape", "Body · Extras"]
    );

    // Cardinality is declared, not inferred from a folder name.
    assert_eq!(
        bundle.groups[1].cardinality,
        Some(apoc_domain::fomod::GroupKind::SelectExactlyOne)
    );
    assert_eq!(
        bundle.groups[2].cardinality,
        Some(apoc_domain::fomod::GroupKind::SelectAny)
    );
}

#[test]
fn a_one_of_group_renders_as_a_radio_set_without_the_wizard_changing() {
    let (bundle, _p, _t) = analyze(&bethesda_rules(), &two_bodies_archive());
    let shape = &bundle.groups[1];

    assert_eq!(shape.options.len(), 2);
    for option in &shape.options {
        assert_eq!(option.select_mode, SelectMode::Exclusive);
        assert!(option.radio_set.is_some());
    }
    // One radio set for the group, which is what makes choosing one clear the
    // other through the planner's existing exclusivity handling.
    assert_eq!(shape.radio_sets().len(), 1);
}

#[test]
fn a_recommended_option_is_marked_without_being_forced() {
    let (bundle, _p, _t) = analyze(&bethesda_rules(), &two_bodies_archive());
    let slim = &bundle.groups[1].options[0];

    assert_eq!(slim.name, "Slim");
    assert!(slim.recommended, "the installer recommends it");
    assert_eq!(
        slim.select_mode,
        SelectMode::Exclusive,
        "recommended is a default, not a decision the user cannot undo"
    );
}

#[test]
fn destinations_are_rooted_where_the_game_profile_says() {
    let (bundle, _p, _t) = analyze(&bethesda_rules(), &two_bodies_archive());
    let all = dests(&bundle);

    assert!(all.contains(&"Data/core.esp".to_string()), "{all:?}");
    assert!(all.contains(&"Data/meshes/body.nif".to_string()), "{all:?}");
    assert!(
        all.contains(&"Data/textures/body.dds".to_string()),
        "{all:?}"
    );
}

#[test]
fn the_same_installer_lands_at_the_game_root_for_a_game_that_says_so() {
    // The prefix belongs to the game, not to the format, and nothing about the
    // engine changes between the two.
    let (bundle, _p, _t) = analyze(&plain_rules(), &two_bodies_archive());
    let all = dests(&bundle);

    assert!(all.contains(&"core.esp".to_string()), "{all:?}");
    assert!(all.contains(&"meshes/body.nif".to_string()), "{all:?}");
}

#[test]
fn a_source_resolves_whatever_case_and_separators_it_was_written_with() {
    let manifest = br#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny"><plugin name="P">
          <files><file source="TEXTURES\Armor\PLATE.DDS" destination="textures/plate.dds"/></files>
        </plugin></group></installStep></installSteps></config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[
            ("fomod/ModuleConfig.xml", manifest),
            ("textures/armor/plate.dds", b"D"),
        ],
    );

    let payload = &bundle.groups[0].options[0].payload;
    assert_eq!(payload.len(), 1);
    // Resolution ignores case; what is stored is the archive's own spelling,
    // because every later lookup matches exactly.
    assert_eq!(payload[0].archive_path, "textures/armor/plate.dds");
    assert_eq!(payload[0].game_rel_path, "textures/plate.dds");
}

#[test]
fn a_staged_fomod_writes_the_files_the_plan_promised() {
    let (bundle, archive, tmp) = analyze(&bethesda_rules(), &two_bodies_archive());
    let staged = tmp.path().join("staging");
    let report = apoc_modengine::stage_bundle(&archive, &bundle, &staged).unwrap();

    assert!(report.files_written >= 4, "{report:?}");
    // Both body variants stage, so switching between them later needs no
    // second visit to the archive.
    let slim = &bundle.groups[1].options[0];
    let muscular = &bundle.groups[1].options[1];
    for option in [slim, muscular] {
        let dir = staged.join(apoc_modengine::plan::option_dir(&option.id));
        assert!(dir.join("Data/meshes/body.nif").is_file(), "{dir:?}");
    }
}

#[test]
fn a_destination_escaping_the_game_directory_refuses_the_whole_archive() {
    let manifest = br#"<config><moduleName>Hostile</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny"><plugin name="P">
          <files><file source="payload.dll" destination="..\..\..\etc\passwd"/></files>
        </plugin></group></installStep></installSteps></config>"#;

    let (result, _p, _t) = analyze_with(
        &plain_rules(),
        &[
            ("fomod/ModuleConfig.xml", manifest),
            ("payload.dll", b"BAD"),
        ],
    );

    match result {
        Err(ModEngineError::UnsafePath(path)) => {
            assert!(path.contains("passwd"), "names the path: {path}");
        }
        other => panic!("a traversal must refuse the archive outright, got {other:?}"),
    }
}

#[test]
fn an_option_whose_files_are_missing_is_shown_disabled_with_the_reason() {
    // Rather than offered, chosen, and then installing nothing at all, which
    // reads as a fault in the manager.
    let manifest = br#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny">
          <plugin name="Present"><files><file source="a.esp"/></files></plugin>
          <plugin name="Absent"><files><file source="never-packed.esp"/></files></plugin>
        </group></installStep></installSteps></config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[("fomod/ModuleConfig.xml", manifest), ("a.esp", b"A")],
    );

    let absent = bundle
        .options()
        .find(|o| o.name == "Absent")
        .expect("the option is still shown");
    assert_eq!(absent.select_mode, SelectMode::Info);
    assert!(!absent.deployable);
    assert!(absent.blocked_reason.is_some());

    let module = bundle.fomod.as_ref().expect("the manifest is carried");
    assert!(
        module
            .warnings
            .iter()
            .any(|w| w.contains("never-packed.esp")),
        "{:?}",
        module.warnings
    );
}

#[test]
fn an_option_the_installer_forbids_is_shown_with_its_reason_rather_than_hidden() {
    let manifest = br#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny">
          <plugin name="Legacy patch">
            <files><file source="legacy.esp"/></files>
            <typeDescriptor><type name="NotUsable"/></typeDescriptor>
          </plugin>
        </group></installStep></installSteps></config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[("fomod/ModuleConfig.xml", manifest), ("legacy.esp", b"L")],
    );

    let option = &bundle.groups[0].options[0];
    assert_eq!(option.select_mode, SelectMode::Info);
    assert!(!option.deployable, "its files are never installed");
    assert!(option.blocked_reason.is_some());
}

#[test]
fn a_required_option_is_forced_whatever_its_group_allows() {
    let manifest = br#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny">
          <plugin name="Core">
            <files><file source="core.esp"/></files>
            <typeDescriptor><type name="Required"/></typeDescriptor>
          </plugin>
        </group></installStep></installSteps></config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[("fomod/ModuleConfig.xml", manifest), ("core.esp", b"C")],
    );
    assert_eq!(bundle.groups[0].options[0].select_mode, SelectMode::Forced);
}

#[test]
fn higher_priority_files_are_ordered_last_within_an_option() {
    let manifest = br#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
        <group name="G" type="SelectAny"><plugin name="P"><files>
          <file source="late.dds" destination="tex/x.dds" priority="5"/>
          <file source="early.dds" destination="tex/x.dds" priority="1"/>
        </files></plugin></group></installStep></installSteps></config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[
            ("fomod/ModuleConfig.xml", manifest),
            ("late.dds", b"L"),
            ("early.dds", b"E"),
        ],
    );

    // The planner takes the last writer within an option, so priority order is
    // payload order.
    let payload = &bundle.groups[0].options[0].payload;
    assert_eq!(payload[0].priority, 1);
    assert_eq!(payload[1].priority, 5);
    assert_eq!(payload[1].archive_path, "late.dds");
}

#[test]
fn a_bare_file_at_the_game_root_is_classified_as_such() {
    let manifest = br#"<config><moduleName>M</moduleName>
        <requiredInstallFiles><file source="dinput8.dll"/></requiredInstallFiles>
        </config>"#;

    let (bundle, _p, _t) = analyze(
        &plain_rules(),
        &[("fomod/ModuleConfig.xml", manifest), ("dinput8.dll", b"D")],
    );
    assert_eq!(
        bundle.groups[0].options[0].payload[0].root,
        DeployRoot::GameRoot
    );
}

#[test]
fn an_installer_whose_sources_are_all_missing_is_reported_as_empty() {
    let manifest = br#"<config><moduleName>M</moduleName>
        <requiredInstallFiles><file source="nothing.esp"/></requiredInstallFiles>
        </config>"#;

    let (result, _p, _t) = analyze_with(
        &plain_rules(),
        &[("fomod/ModuleConfig.xml", manifest), ("readme.txt", b"R")],
    );
    // The required-files group is still built, but it deploys nothing, so the
    // wizard opens on an installer that would install nothing at all.
    let bundle = result.expect("analysing succeeds");
    assert!(bundle.deployable_options().next().is_none());
}
