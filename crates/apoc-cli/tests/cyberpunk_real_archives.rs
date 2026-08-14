//! The five real Cyberpunk mods, checked against where their files must land.
//!
//! Every other Cyberpunk test in this repository builds its own archive, which
//! means it proves the engine agrees with the shape whoever wrote the test had
//! in mind. That is worth having and it is not the same as being right. These
//! run against archives people downloaded: Cyber Engine Tweaks, RED4ext,
//! redscript, an asset mod and a scripted mod, which between them are the whole
//! Cyberpunk stack.
//!
//! The archives are other people's work and one of them is sixty-two megabytes,
//! so they are not committed and every test here skips, loudly, when they are
//! absent. Put them in `scratch/test-mods/cyberpunk/` or point `APOC_CP77_MODS`
//! somewhere else.
//!
//! Where a mod ships few files the whole destination list is asserted, because
//! the list is short enough to read and a change to any of it is worth stopping
//! for. Where a mod ships many, the count and the paths that carry an argument
//! are asserted instead: pinning all nineteen of Cyber Engine Tweaks' files
//! would fail on the next release for no reason anybody could act on.

use apoc_domain::{ModBundle, SelectMode};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_modengine::GameRules;
use std::path::PathBuf;

mod common;

const MODS_ENV: &str = "APOC_CP77_MODS";
const MODS_SUBDIR: &str = "test-mods/cyberpunk";

fn cp77_profile() -> apoc_domain::GameProfile {
    LocalBuiltin::new()
        .get("cyberpunk-2077")
        .expect("the cyberpunk profile ships")
}

/// Rules taken from the shipped profile, never written here: a profile change
/// that would break one of these mods has to fail in this file.
fn cp77_rules() -> GameRules {
    GameRules::from_profile(&cp77_profile())
}

fn archive(fragments: &[&str]) -> Option<PathBuf> {
    common::find_archive(MODS_ENV, MODS_SUBDIR, fragments)
}

/// Analyse one archive and return it with its destinations, sorted.
fn analyse(path: &std::path::Path) -> (ModBundle, Vec<String>) {
    let bundle = apoc_modengine::analyze_archive_with(path, &cp77_rules())
        .expect("a real Cyberpunk archive analyses");
    let mut dests: Vec<String> = bundle
        .deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.game_rel_path.clone())
        .collect();
    dests.sort();
    (bundle, dests)
}

/// Every mod here is a single payload with nothing to choose, which is the
/// honest answer to whether Cyberpunk mods offer install options: these do not.
/// A mod that grew options would fail here rather than quietly installing a
/// default nobody picked.
fn assert_single_forced_option(bundle: &ModBundle, name: &str) {
    let options: Vec<_> = bundle.options().collect();
    assert_eq!(options.len(), 1, "{name} should offer exactly one option");
    assert_eq!(
        options[0].select_mode,
        SelectMode::Forced,
        "{name}'s only option should be forced, not a choice"
    );
    assert!(
        bundle.groups.iter().all(|g| g.radio_sets().is_empty()),
        "{name} should present no choice sets"
    );
}

/// Nothing may land outside a directory the profile declares.
fn assert_within_declared_targets(dests: &[String], name: &str) {
    let profile = cp77_profile();
    for dest in dests {
        let root = dest.split('/').next().unwrap_or_default();
        assert!(
            profile
                .deploy_targets
                .iter()
                .any(|t| t.source.eq_ignore_ascii_case(root)),
            "{name} would write outside every declared deploy target: {dest}"
        );
    }
}

macro_rules! skip_unless {
    ($fragments:expr, $what:expr) => {
        match archive($fragments) {
            Some(path) => path,
            None => {
                eprintln!("{}", common::describe_missing($what, MODS_ENV, MODS_SUBDIR));
                return;
            }
        }
    };
}

#[test]
fn cyber_engine_tweaks_installs_entirely_under_bin_x64() {
    let path = skip_unless!(&["cet"], "Cyber Engine Tweaks");
    let (bundle, dests) = analyse(&path);

    assert_single_forced_option(&bundle, "Cyber Engine Tweaks");
    assert_within_declared_targets(&dests, "Cyber Engine Tweaks");
    assert_eq!(dests.len(), 19, "CET 1.37.1 ships nineteen files");

    // Everything belongs beside the game binary. A single file escaping into the
    // game root would be a loader that never loads.
    assert!(
        dests.iter().all(|d| d.starts_with("bin/x64/")),
        "something escaped bin/x64: {dests:?}"
    );

    // The proxy, and the ASI the proxy loads.
    assert!(dests.contains(&"bin/x64/version.dll".to_string()));
    assert!(dests.contains(&"bin/x64/plugins/cyber_engine_tweaks.asi".to_string()));

    // The `.asi` sits beside a directory of the same name, and the casing list
    // holds that directory. Whole-segment matching is what keeps the file a file.
    assert!(
        dests
            .iter()
            .any(|d| d.starts_with("bin/x64/plugins/cyber_engine_tweaks/")),
        "the plugin directory is missing: {dests:?}"
    );

    // Folded, because CET reads them by name.
    assert!(dests
        .iter()
        .any(|d| d.starts_with("bin/x64/plugins/cyber_engine_tweaks/fonts/")));
    assert!(dests
        .iter()
        .any(|d| d.starts_with("bin/x64/plugins/cyber_engine_tweaks/tweakdb/")));

    // Left alone, because they are vendored inside CET and nothing else ships
    // them. This is the control for the casing list's second rule.
    assert!(
        dests.contains(
            &"bin/x64/plugins/cyber_engine_tweaks/scripts/IconGlyphs/icons.lua".to_string()
        ),
        "IconGlyphs was folded when it should have been left alone: {dests:?}"
    );
}

#[test]
fn red4ext_arrives_as_a_proxy_beside_its_framework() {
    let path = skip_unless!(&["red4ext"], "RED4ext");
    let (bundle, dests) = analyse(&path);

    assert_single_forced_option(&bundle, "RED4ext");
    assert_within_declared_targets(&dests, "RED4ext");
    assert_eq!(
        dests,
        vec![
            "bin/x64/winmm.dll",
            "red4ext/LICENSE.txt",
            "red4ext/RED4ext.dll",
            "red4ext/THIRD_PARTY_LICENSES.txt",
        ]
    );

    // The release ships full paths, so it must be read as loose roots. The
    // loader model exists for a bare DLL with no directory around it, and
    // applying it here would flatten `red4ext/` into the game root.
    assert_eq!(
        bundle.installer_model,
        apoc_domain::InstallerModel::LooseRoots
    );

    // Read from the profile rather than written here, so moving the proxy in the
    // TOML fails in this test rather than silently changing where it lands.
    let profile = cp77_profile();
    let proxy = profile
        .loader
        .as_ref()
        .and_then(|l| l.proxy_dll.clone())
        .expect("the profile declares a proxy DLL");
    assert!(
        dests.contains(&proxy),
        "the release does not put the proxy where the profile says: {dests:?}"
    );

    // Mixed casing the author chose, on a file nothing reads by name.
    assert!(dests.contains(&"red4ext/RED4ext.dll".to_string()));
}

#[test]
fn redscript_spans_the_engine_and_r6_trees() {
    let path = skip_unless!(&["redscript"], "redscript");
    let (bundle, dests) = analyse(&path);

    assert_single_forced_option(&bundle, "redscript");
    assert_within_declared_targets(&dests, "redscript");
    assert_eq!(
        dests,
        vec![
            "engine/config/base/scripts.ini",
            "engine/tools/scc.exe",
            "engine/tools/scc_lib.dll",
            "r6/config/cybercmd/scc.toml",
        ]
    );

    // Eleven entries in the zip, four of them files: the seven directory entries
    // must contribute nothing rather than becoming empty payloads.
    assert_eq!(dests.len(), 4);

    // This is the one file among all five mods that the game itself ships, so
    // installing redscript replaces it. `cyberpunk_deploy.rs` is where that is
    // proved to be vaulted and restorable.
    assert!(dests.contains(&"engine/config/base/scripts.ini".to_string()));
}

#[test]
fn an_asset_mod_is_one_archive_file() {
    let path = skip_unless!(&["vanilla refit"], "Vanilla Refit");
    let (bundle, dests) = analyse(&path);

    assert_single_forced_option(&bundle, "Vanilla Refit");
    assert_within_declared_targets(&dests, "Vanilla Refit");
    assert_eq!(dests.len(), 1, "the whole mod is a single .archive");
    assert!(dests[0].starts_with("archive/pc/mod/"));
    assert!(dests[0].ends_with(".archive"));

    // Sixty-two megabytes in one payload. Worth asserting because sizing feeds
    // the interface's totals and a truncated size would show as a mod that
    // appears to install nothing much.
    let bytes: u64 = bundle
        .deployable_options()
        .flat_map(|o| o.payload.iter())
        .map(|f| f.size)
        .sum();
    assert!(
        bytes > 60 * 1024 * 1024,
        "the payload lost its size: {bytes} bytes"
    );
}

#[test]
fn a_scripted_mod_keeps_its_script_tree_and_its_xl() {
    let path = skip_unless!(&["virtual car dealer"], "Virtual Car Dealer");
    let (bundle, dests) = analyse(&path);

    assert_single_forced_option(&bundle, "Virtual Car Dealer");
    assert_within_declared_targets(&dests, "Virtual Car Dealer");
    assert_eq!(
        dests.len(),
        21,
        "two archive files, one hint, eighteen scripts"
    );

    // ArchiveXL's sidecar travels beside the archive it extends. Cyberpunk
    // declares no patch chain, so neither may be renamed into one.
    assert!(dests.contains(&"archive/pc/mod/VirtualCarDealer.archive".to_string()));
    assert!(dests.contains(&"archive/pc/mod/VirtualCarDealer.archive.xl".to_string()));
    assert!(cp77_profile().pak_chain.is_none());
    assert!(
        !dests.iter().any(|d| d.contains("patch_")),
        "something was renamed into a patch chain this game does not have"
    );

    // The framework directory, folded to the spelling redscript reads.
    assert!(dests.contains(&"r6/config/redsUserHints/VirtualCarDealer.toml".to_string()));

    // The author's own tree, untouched: eighteen scripts under a folder they
    // named, including a subfolder whose casing nothing may fold.
    let scripts: Vec<&String> = dests.iter().filter(|d| d.ends_with(".reds")).collect();
    assert_eq!(scripts.len(), 18);
    assert!(scripts
        .iter()
        .all(|d| d.starts_with("r6/scripts/VirtualCarDealer/")));
    assert!(
        dests
            .iter()
            .any(|d| d.starts_with("r6/scripts/VirtualCarDealer/UI/")),
        "the author's UI folder was folded: {dests:?}"
    );
}

#[test]
fn a_framework_installs_the_licence_it_is_required_to_carry() {
    // Not an accident worth tidying away. RED4ext ships its licence text because
    // its licence asks that the text travel with the binary, and a mod manager
    // that quietly dropped it would be making that omission on the user's
    // behalf. Everything under a declared payload root deploys, which is a rule
    // somebody can predict; a list of file names worth skipping is one that
    // eventually eats something real.
    let path = skip_unless!(&["red4ext"], "RED4ext");
    let (_, dests) = analyse(&path);

    assert!(dests.contains(&"red4ext/LICENSE.txt".to_string()));
    assert!(dests.contains(&"red4ext/THIRD_PARTY_LICENSES.txt".to_string()));
}

#[test]
fn no_two_of_these_mods_want_the_same_file() {
    // The five together are a realistic install: two loaders, a script compiler,
    // an asset mod and a scripted mod. Nothing in that set should contest a
    // path, so a conflict here means a rewrap or casing rule has started
    // rerouting one mod's files on top of another's.
    let mut all: Vec<String> = Vec::new();
    let mut found = 0;
    for fragments in [
        &["cet"][..],
        &["red4ext"][..],
        &["redscript"][..],
        &["vanilla refit"][..],
        &["virtual car dealer"][..],
    ] {
        let Some(path) = archive(fragments) else {
            continue;
        };
        found += 1;
        all.extend(analyse(&path).1);
    }
    if found == 0 {
        eprintln!(
            "{}",
            common::describe_missing("any Cyberpunk", MODS_ENV, MODS_SUBDIR)
        );
        return;
    }

    let mut sorted = all.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    assert_eq!(
        before,
        sorted.len(),
        "two of these mods want to write the same file"
    );

    if found == 5 {
        assert_eq!(before, 49, "the five mods together write forty-nine files");
    }
}
