//! Deploying a Creation Engine mod, end to end, through the plugin list.
//!
//! The pieces are unit-tested where they live; what this covers is that they
//! agree with each other. Every claim is checked against the file the *game*
//! reads, not against a return value, because that file is the only thing the
//! game will ever look at.

use apoc_deploy::journal::JournalOp;
use apoc_deploy::plugin_list::{self, PluginListTarget};
use apoc_deploy::{DeployContext, PluginListOutcome};
use apoc_domain::plugins::{MasterProblem, PluginKind};
use apoc_domain::{GameProfile, LoadOrderPolicy, PluginActivation, PluginListSpec, SteamDetection};
use apoc_modengine::GameRules;
use std::fs;
use std::path::{Path, PathBuf};

/// A Skyrim-shaped profile, built here rather than loaded so the test says what
/// it depends on.
fn profile() -> GameProfile {
    GameProfile {
        id: "skyrim-special-edition".into(),
        name: "Skyrim".into(),
        engine: apoc_domain::Engine::Creation,
        detection: SteamDetection {
            steam_app_id: 489830,
            executable: None,
        },
        nexus_domain: None,
        load_order: LoadOrderPolicy::Explicit,
        conflict_scope: apoc_domain::ConflictScope::PerRelativePath,
        case_sensitive: true,
        deploy_targets: vec![apoc_domain::DeployTarget {
            source: "Data".into(),
            target: "Data".into(),
        }],
        formats: vec!["loose-roots".into()],
        fomod: None,
        plugin_extensions: vec!["esp".into(), "esm".into(), "esl".into()],
        rewrap: vec![],
        canonical_case: vec!["Data".into()],
        loader: None,
        pak_chain: None,
        plugin_list: Some(PluginListSpec {
            dir: "Skyrim Special Edition".into(),
            plugins_file: "plugins.txt".into(),
            load_order_file: Some("loadorder.txt".into()),
            activation: PluginActivation::Asterisk,
            implicit: vec!["Skyrim.esm".into(), "Update.esm".into()],
        }),
    }
}

/// A minimal plugin file: a `TES4` record with the masters given.
fn plugin_bytes(master_flag: bool, masters: &[&str]) -> Vec<u8> {
    let mut data = Vec::new();
    for m in masters {
        let mut body = m.as_bytes().to_vec();
        body.push(0);
        data.extend_from_slice(b"MAST");
        data.extend_from_slice(&(body.len() as u16).to_le_bytes());
        data.extend_from_slice(&body);
        data.extend_from_slice(b"DATA");
        data.extend_from_slice(&8u16.to_le_bytes());
        data.extend_from_slice(&[0u8; 8]);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"TES4");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(if master_flag { 1u32 } else { 0 }).to_le_bytes());
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(&data);
    out
}

struct World {
    _dir: tempfile::TempDir,
    ctx: DeployContext,
    target: PluginListTarget,
    game_dir: PathBuf,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    let prefix = dir.path().join("pfx");
    fs::create_dir_all(game_dir.join("Data")).unwrap();

    let target = PluginListTarget::resolve(&prefix, profile().plugin_list.as_ref().unwrap())
        .expect("the profile resolves");
    fs::create_dir_all(&target.dir).unwrap();

    let ctx = DeployContext {
        game_id: "skyrim-special-edition".into(),
        game_dir: game_dir.clone(),
        staging_dir: dir.path().join("staging"),
        vault_dir: dir.path().join("vault"),
        journal_dir: dir.path().join("journal"),
        ladder: Default::default(),
        pak_chain: None,
        copy_only_paths: Vec::new(),
    };
    World {
        _dir: dir,
        ctx,
        target,
        game_dir,
    }
}

/// Put a plugin where a deployment would have put it.
fn deploy_plugin(game_dir: &Path, rel: &str, master: bool, masters: &[&str]) {
    let path = game_dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, plugin_bytes(master, masters)).unwrap();
}

fn rules() -> GameRules {
    GameRules::from_profile(&profile())
}

fn fold_in(w: &World, deployed: &[&str]) -> (PluginListOutcome, apoc_deploy::journal::Journal) {
    let paths: Vec<String> = deployed.iter().map(|s| s.to_string()).collect();
    let entries = apoc_modengine::plugins::deployed_entries(&w.game_dir, &paths, &rules());

    let mut journal = apoc_deploy::journal::Journal::create(
        &w.ctx.journal_dir,
        apoc_deploy::journal::DeploymentHeader {
            id: apoc_deploy::journal::new_deployment_id(),
            game_id: w.ctx.game_id.clone(),
            bundle_name: "test".into(),
            created_at: 0,
            game_dir: w.game_dir.to_string_lossy().into_owned(),
        },
    )
    .unwrap();

    let outcome =
        apoc_deploy::provision_plugin_list(&w.ctx, &mut journal, &w.target, entries).unwrap();
    (outcome, journal)
}

fn list(w: &World) -> String {
    fs::read_to_string(&w.target.plugins_file).unwrap()
}

#[test]
fn a_deployed_plugin_reaches_the_list_switched_on() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Ordinator.esp", false, &["Skyrim.esm"]);

    let (outcome, _j) = fold_in(&w, &["Data/Ordinator.esp"]);

    assert_eq!(outcome.added, vec!["Ordinator.esp"]);
    assert!(outcome.written);
    assert!(
        outcome.violations.is_empty(),
        "Skyrim.esm is loaded implicitly, so the dependency holds: {:?}",
        outcome.violations
    );
    assert_eq!(list(&w), "*Skyrim.esm\n*Update.esm\n*Ordinator.esp\n");
}

/// The whole promise of the chosen behaviour: what the person curated in
/// another tool is still exactly what it was, with the new plugin after it.
#[test]
fn an_order_curated_elsewhere_is_kept_and_only_added_to() {
    let w = world();
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*ZDeliberatelyLast.esp\nADisabledOnPurpose.esp\n",
    )
    .unwrap();
    deploy_plugin(&w.game_dir, "Data/New.esp", false, &[]);

    fold_in(&w, &["Data/New.esp"]);

    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\n*ZDeliberatelyLast.esp\nADisabledOnPurpose.esp\n*New.esp\n",
        "nothing moved, nothing was switched, the new one is last"
    );
}

/// A mod shipping a master gets it into the master block, not after the
/// plugins that will name it.
#[test]
fn a_deployed_master_joins_the_master_block() {
    let w = world();
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Mod.esp\n",
    )
    .unwrap();
    deploy_plugin(&w.game_dir, "Data/NewLand.esm", true, &["Skyrim.esm"]);

    fold_in(&w, &["Data/NewLand.esm"]);

    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\n*NewLand.esm\n*Mod.esp\n"
    );
}

/// Reported, never repaired: the plugin names a master that is nowhere, and
/// the list is still written so the rest of the mod works.
#[test]
fn a_missing_master_is_reported_and_the_list_is_still_written() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Patch.esp", false, &["NotInstalled.esm"]);

    let (outcome, _j) = fold_in(&w, &["Data/Patch.esp"]);

    assert_eq!(outcome.violations.len(), 1);
    assert_eq!(outcome.violations[0].plugin, "Patch.esp");
    assert_eq!(outcome.violations[0].master, "NotInstalled.esm");
    assert_eq!(outcome.violations[0].reason, MasterProblem::Missing);
    assert!(list(&w).contains("*Patch.esp"), "the file still lands");
}

#[test]
fn a_file_that_is_not_a_plugin_never_reaches_the_list() {
    let w = world();
    let mesh = w.game_dir.join("Data/meshes/x.nif");
    fs::create_dir_all(mesh.parent().unwrap()).unwrap();
    fs::write(&mesh, b"not a plugin").unwrap();

    let (outcome, _j) = fold_in(&w, &["Data/meshes/x.nif"]);

    assert!(outcome.added.is_empty());
    assert!(!outcome.written, "there was nothing to write");
    assert!(
        !w.target.plugins_file.exists(),
        "a mod with no plugins does not conjure a list the game writes for itself"
    );
}

/// A texture pack renamed to `.esp` is not a plugin, but it is a plugin *file*
/// by the profile's extensions. It joins the list with no known constraints
/// rather than failing the deploy.
#[test]
fn a_plugin_whose_header_will_not_parse_still_installs() {
    let w = world();
    let path = w.game_dir.join("Data/Broken.esp");
    fs::write(&path, b"DDS |certainly not a TES4 record").unwrap();

    let (outcome, _j) = fold_in(&w, &["Data/Broken.esp"]);

    assert_eq!(outcome.added, vec!["Broken.esp"]);
    assert!(outcome.violations.is_empty(), "nothing is known to defend");
    assert!(list(&w).contains("*Broken.esp"));
}

/// Invariant 1 and 2, at the level the user meets them: the original is
/// recoverable and the record of the write survives the write.
#[test]
fn the_previous_list_is_vaulted_and_the_write_is_journalled() {
    let w = world();
    fs::write(&w.target.plugins_file, "*Skyrim.esm\n*Update.esm\n").unwrap();
    deploy_plugin(&w.game_dir, "Data/New.esp", false, &[]);

    let (_outcome, journal) = fold_in(&w, &["Data/New.esp"]);

    let ops: Vec<&JournalOp> = journal
        .ops()
        .iter()
        .filter(|op| matches!(op, JournalOp::PluginListWritten { .. }))
        .collect();
    assert_eq!(ops.len(), 2, "plugins.txt and loadorder.txt");

    let JournalOp::PluginListWritten {
        original_vault_key,
        path,
        sha256,
    } = ops[0]
    else {
        unreachable!()
    };
    let key = original_vault_key
        .as_deref()
        .expect("there was a list to keep");

    // Invariant 3: it goes back, byte for byte.
    assert!(
        plugin_list::rollback_one(Path::new(path), Some(key), sha256, &w.ctx.vault_dir).unwrap()
    );
    assert_eq!(list(&w), "*Skyrim.esm\n*Update.esm\n");
}

/// The case that matters most, at the end of the whole path: the user opens
/// their own load-order tool between the deploy and the rollback.
#[test]
fn a_rollback_leaves_a_list_the_user_has_since_curated() {
    let w = world();
    fs::write(&w.target.plugins_file, "*Skyrim.esm\n*Update.esm\n").unwrap();
    deploy_plugin(&w.game_dir, "Data/New.esp", false, &[]);
    let (_outcome, journal) = fold_in(&w, &["Data/New.esp"]);

    let theirs = "*Skyrim.esm\n*Update.esm\n*New.esp\n*SomethingTheyAdded.esp\n";
    fs::write(&w.target.plugins_file, theirs).unwrap();

    let report = apoc_deploy::rollback(&w.ctx, &journal, None);

    assert_eq!(list(&w), theirs, "their curation survives");
    assert!(
        report
            .skipped_modified
            .iter()
            .any(|p| p.ends_with("plugins.txt")),
        "and it is reported rather than silently skipped: {report:?}"
    );
}

#[test]
fn deploying_the_same_plugin_twice_changes_nothing_the_second_time() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Mod.esp", false, &[]);
    fold_in(&w, &["Data/Mod.esp"]);
    let after_first = list(&w);

    let (outcome, _j) = fold_in(&w, &["Data/Mod.esp"]);

    assert!(outcome.added.is_empty(), "it was already there");
    assert!(!outcome.written, "an unchanged list is not rewritten");
    assert_eq!(list(&w), after_first);
}

/// Two mods can deploy a file of the same name; one of them won the conflict
/// and is the file on disk. The list names it once.
#[test]
fn a_plugin_deployed_by_two_mods_appears_once() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Shared.esp", false, &[]);

    let (outcome, _j) = fold_in(&w, &["Data/Shared.esp", "Data/Shared.esp"]);

    assert_eq!(outcome.added, vec!["Shared.esp"]);
    assert_eq!(list(&w).matches("Shared.esp").count(), 1);
}

#[test]
fn the_kind_comes_from_the_flag_not_the_name() {
    let w = world();
    // A `.esp` carrying the master flag: the engine loads it in the master
    // block, so the list must put it there.
    deploy_plugin(&w.game_dir, "Data/SecretlyAMaster.esp", true, &[]);
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Plain.esp\n",
    )
    .unwrap();

    fold_in(&w, &["Data/SecretlyAMaster.esp"]);

    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\n*SecretlyAMaster.esp\n*Plain.esp\n"
    );
    assert_eq!(
        apoc_modengine::plugins::deployed_entries(
            &w.game_dir,
            &["Data/SecretlyAMaster.esp".to_string()],
            &rules()
        )[0]
        .kind,
        PluginKind::Master
    );
}

/* ======================================================= editing the order ===

The Plugins screen. A deploy may only append to somebody's list; moving an
entry that is already in it is the user's own action, and these cover the
sequence the commands perform: read what is on disk, describe it from the
files it names, apply one domain call, write, journal.

The commands themselves live in the Tauri crate and need a running
application state. What is worth proving is the engine sequence underneath
them, which is where every invariant actually lives.
========================================================================== */

/// What `plugin_cmds::read_described` does: the list as the user has it, with
/// each entry's real kind and masters read from the file the game will load.
fn describe(w: &World) -> apoc_domain::plugins::PluginOrder {
    let mut order = plugin_list::read(&w.target);
    let names: Vec<String> = order.entries().iter().map(|e| e.name.clone()).collect();
    let roots = vec!["Data".to_string()];
    order.append_new(apoc_modengine::plugins::entries_for_names(
        &w.game_dir,
        &names,
        &roots,
        &rules(),
    ));
    // Held the way the game reads it, so the index the screen shows and the
    // index a move names are the same number.
    apoc_domain::plugins::PluginOrder::adopt(
        order.as_the_game_reads_it().into_iter().cloned().collect(),
    )
}

/// What `plugin_cmds::commit` does: write, then journal, in that order.
fn commit(w: &World, order: &apoc_domain::plugins::PluginOrder) -> apoc_deploy::journal::Journal {
    let ops = plugin_list::write(&w.target, &w.ctx.vault_dir, order).unwrap();
    let mut journal = apoc_deploy::journal::Journal::create(
        &w.ctx.journal_dir,
        apoc_deploy::journal::DeploymentHeader {
            id: apoc_deploy::journal::new_deployment_id(),
            game_id: w.ctx.game_id.clone(),
            bundle_name: "plugin order".into(),
            created_at: 0,
            game_dir: w.game_dir.to_string_lossy().into_owned(),
        },
    )
    .unwrap();
    for op in ops {
        journal.append(op).unwrap();
    }
    journal
}

/// A list with two masters and two ordinary plugins, all present on disk.
fn curated(w: &World) {
    deploy_plugin(&w.game_dir, "Data/Armour.esp", false, &[]);
    deploy_plugin(&w.game_dir, "Data/Weapons.esp", false, &[]);
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Armour.esp\n*Weapons.esp\n",
    )
    .unwrap();
}

#[test]
fn a_move_reaches_the_file_the_game_reads() {
    let w = world();
    curated(&w);

    let mut order = describe(&w);
    assert!(
        order.move_to("Weapons.esp", 2),
        "Weapons moves above Armour"
    );
    commit(&w, &order);

    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\n*Weapons.esp\n*Armour.esp\n",
        "the next thing to read this file is the game"
    );
}

/// Invariants 1 and 2 for an edit, which is the whole reason an edit journals
/// at all: the list the user had is recoverable, and the record of replacing it
/// survives the replacing.
#[test]
fn an_edit_vaults_the_previous_list_and_journals_the_write() {
    let w = world();
    curated(&w);
    let before = list(&w);

    let mut order = describe(&w);
    order.move_to("Weapons.esp", 2);
    let journal = commit(&w, &order);

    let op = journal
        .ops()
        .iter()
        .find(|op| matches!(op, JournalOp::PluginListWritten { .. }))
        .expect("the write is recorded");
    let JournalOp::PluginListWritten {
        original_vault_key, ..
    } = op
    else {
        unreachable!()
    };

    let key = original_vault_key
        .as_ref()
        .expect("there was a list before this edit");

    // Asked of the vault rather than of a path spelled out here, so this keeps
    // proving what it claims if the vault's layout ever changes.
    let vault = apoc_deploy::vault::Vault::new(&w.ctx.vault_dir);
    assert!(vault.contains(key), "the previous list is in the vault");

    let scratch = w.ctx.staging_dir.join("recovered.txt");
    fs::create_dir_all(&w.ctx.staging_dir).unwrap();
    vault.restore(key, &scratch).unwrap();
    assert_eq!(
        fs::read_to_string(scratch).unwrap(),
        before,
        "the order the user had is still recoverable byte for byte"
    );
}

/// An edit is reversible on exactly the terms a deployment is, including the
/// refusal that matters most here: between the edit and the undo, the user may
/// have arranged the list again in another tool.
#[test]
fn an_edit_can_be_rolled_back_and_refuses_to_when_it_should() {
    let w = world();
    curated(&w);
    let before = list(&w);

    let mut order = describe(&w);
    order.move_to("Weapons.esp", 2);
    let journal = commit(&w, &order);

    let report = apoc_deploy::rollback(&w.ctx, &journal, None);
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(list(&w), before, "the arrangement is back");

    // And again, with somebody else's hand in between.
    let mut order = describe(&w);
    order.move_to("Weapons.esp", 2);
    let journal = commit(&w, &order);
    let theirs = "*Skyrim.esm\n*Update.esm\n*TheirOwnIdea.esp\n";
    fs::write(&w.target.plugins_file, theirs).unwrap();

    let report = apoc_deploy::rollback(&w.ctx, &journal, None);
    assert_eq!(list(&w), theirs, "their curation survives");
    assert!(
        report
            .skipped_modified
            .iter()
            .any(|p| p.ends_with("plugins.txt")),
        "and is reported rather than silently skipped: {report:?}"
    );
}

/// Dragging a plugin back where it started must not write a file, or the
/// journal directory fills with records that nothing happened.
#[test]
fn a_move_that_changes_nothing_writes_nothing() {
    let w = world();
    curated(&w);
    // Both files as a previous edit would have left them, so what this measures
    // is the second write and not the creation of `loadorder.txt`.
    commit(&w, &describe(&w));

    let mut order = describe(&w);
    assert!(!order.move_to("Armour.esp", 2), "it is already at 2");
    let ops = plugin_list::write(&w.target, &w.ctx.vault_dir, &order).unwrap();

    assert!(ops.is_empty(), "an unchanged list is not rewritten");
}

/// The masters this reports are read out of the deployed files, which is the
/// only place they exist — a list file carries names and switches and nothing
/// else. Without that the screen would show no problems, ever.
#[test]
fn the_screen_sees_a_violation_the_list_file_alone_cannot_express() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Base.esp", false, &[]);
    deploy_plugin(&w.game_dir, "Data/Patch.esp", false, &["Base.esp"]);
    // The patch is written above the thing it depends on.
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Patch.esp\n*Base.esp\n",
    )
    .unwrap();

    let plain = plugin_list::read(&w.target);
    assert!(
        plain.violations().is_empty(),
        "the file alone knows of no dependency"
    );

    let described = describe(&w);
    let violations = described.violations();
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].plugin, "Patch.esp");
    assert_eq!(violations[0].master, "Base.esp");
    assert_eq!(violations[0].reason, MasterProblem::LoadsTooLate);
}

/// The targeted fix: move the master to where the plugin that needs it sits.
/// The master moves, not the dependant — moving the dependant down would
/// satisfy this one constraint by changing its relationship to everything else.
#[test]
fn the_offered_fix_resolves_the_violation_it_names() {
    let w = world();
    deploy_plugin(&w.game_dir, "Data/Base.esp", false, &[]);
    deploy_plugin(&w.game_dir, "Data/Patch.esp", false, &["Base.esp"]);
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Patch.esp\n*Base.esp\n",
    )
    .unwrap();

    let mut order = describe(&w);
    let v = order.violations().remove(0);
    // Exactly what `fix_for` computes: the master goes to the dependant's index.
    let to = order.position(&v.plugin).unwrap();
    assert!(order.move_to(&v.master, to));
    commit(&w, &order);

    assert!(
        describe(&w).violations().is_empty(),
        "still broken: {}",
        list(&w)
    );
    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\n*Base.esp\n*Patch.esp\n"
    );
}

/// Switching a plugin off is the other edit, and it is not the same as removing
/// it: the line stays, so the user can switch it back on.
#[test]
fn switching_a_plugin_off_keeps_its_place_in_the_list() {
    let w = world();
    curated(&w);

    let mut order = describe(&w);
    assert!(order.set_enabled("Armour.esp", false));
    commit(&w, &order);

    assert_eq!(
        list(&w),
        "*Skyrim.esm\n*Update.esm\nArmour.esp\n*Weapons.esp\n",
        "still third, no longer loaded"
    );
}

/// A line naming a plugin whose mod was uninstalled. The game ignores it; the
/// screen has to show it, because otherwise nobody can ever remove it.
#[test]
fn a_line_whose_file_is_gone_stays_in_the_list() {
    let w = world();
    fs::write(
        &w.target.plugins_file,
        "*Skyrim.esm\n*Update.esm\n*Vanished.esp\n",
    )
    .unwrap();

    let order = describe(&w);

    let entry = order.get("Vanished.esp").expect("still listed");
    assert!(entry.enabled);
    assert!(
        entry.masters.is_empty(),
        "nothing is known about a file that is not there"
    );
}
