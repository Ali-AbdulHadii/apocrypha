//! `apoc plan`: where would this mod's files actually go?
//!
//! `analyze` answers "what is in this archive". That is a different question
//! from "what would installing it do to my game", and until now the only way to
//! ask the second one was to write Rust or to install the mod and look. This
//! runs the real pipeline — analyse, stage, select, plan, dry run — and prints
//! the destinations.
//!
//! The game id is positional and required, which is the whole point. `analyze`
//! takes no game and therefore silently answers with RE Engine's rules, so
//! against a Cyberpunk mod it reports nothing deployable. A command that cannot
//! be run without naming the game cannot be wrong about the game.
//!
//! `apply` is never called from here. Staging is real, because that is what
//! proves each payload resolves to an entry that can actually be extracted, but
//! it happens in a temporary directory alongside an empty stand-in for the game
//! and both are gone when the command returns. There is no flag that makes this
//! write to a game directory; that is the structural version of promising it
//! did not install anything.

use apoc_deploy::{dry_run, place::Ladder, DeployContext};
use apoc_domain::{GameProfile, ModBundle, Selection};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use apoc_modengine::GameRules;
use std::path::Path;

use crate::human_size;

pub fn run(game_id: &str, archive: &Path, forced_only: bool) -> Result<(), String> {
    let profile = LocalBuiltin::new()
        .get(game_id)
        .map_err(|e| e.to_string())?;
    let rules = GameRules::from_profile(&profile);

    let bundle =
        apoc_modengine::analyze_archive_with(archive, &rules).map_err(|e| e.to_string())?;

    println!("Mod  : {}", bundle.name);
    println!(
        "       {}  by {}  [{:?}]",
        bundle.version.as_deref().unwrap_or("v?"),
        bundle.author.as_deref().unwrap_or("unknown"),
        bundle.installer_model
    );
    println!("Game : {} ({})", profile.name, profile.id);

    let selection = build_selection(&bundle, forced_only);
    let plan = apoc_modengine::plan_deployment(&bundle, &selection);

    if plan.files.is_empty() {
        return report_nothing_to_install(&profile, &rules, archive);
    }

    // Stage for real, into a directory that does not survive this function.
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let staging = tmp.path().join("staging");
    apoc_modengine::stage_bundle(archive, &bundle, &staging).map_err(|e| e.to_string())?;

    let game_dir = tmp.path().join("game");
    std::fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;

    let ctx = DeployContext {
        game_id: profile.id.clone(),
        game_dir,
        staging_dir: staging,
        vault_dir: tmp.path().join("vault"),
        journal_dir: tmp.path().join("journal"),
        ladder: Ladder::default(),
        pak_chain: profile.pak_chain.clone(),
        copy_only_paths: DeployContext::copy_only_from(&profile),
    };
    let dr = dry_run(&ctx, &plan).map_err(|e| e.to_string())?;

    println!(
        "\n{} files, {} — {}",
        plan.file_count(),
        human_size(plan.total_size()),
        if forced_only {
            "required files only"
        } else {
            "default selection"
        }
    );
    if !plan.issues.is_empty() {
        for issue in &plan.issues {
            println!("!! {issue:?}");
        }
    }

    println!();
    for f in &plan.files {
        println!("  {{game}}/{}", f.game_rel_path);
    }

    // A staged file the plan references but that is not on disk means the
    // archive and the plan disagree, which is the bug that installs short.
    if !dr.missing.is_empty() {
        println!("\n!! {} payload files did not stage:", dr.missing.len());
        for m in &dr.missing {
            println!("     {m}");
        }
        return Err("this mod would install incompletely".to_string());
    }

    println!("\nNothing was installed: this ran against a temporary directory.");
    Ok(())
}

/// The branch this command is worth building for.
///
/// "The mod installed and did nothing" is otherwise an afternoon of guessing.
/// Both halves are needed: what the game reads, and what the archive has.
fn report_nothing_to_install(
    profile: &GameProfile,
    rules: &GameRules,
    archive: &Path,
) -> Result<(), String> {
    println!(
        "\n!! Nothing to install: no file in this archive matched a payload root for this game."
    );
    println!(
        "   Payload roots for {}: {}",
        profile.id,
        rules.payload_roots.join(", ")
    );
    match apoc_modengine::archive_roots(archive) {
        Ok(roots) if !roots.is_empty() => {
            println!("   Archive roots seen: {}", roots.join(", "))
        }
        Ok(_) => println!("   Archive roots seen: none — the archive holds no files"),
        Err(e) => println!("   Archive roots could not be read: {e}"),
    }
    if !rules.root_files.is_empty() {
        let names: Vec<&str> = rules.root_files.iter().map(|(n, _)| n.as_str()).collect();
        println!(
            "   Loader files recognised at the archive root: {}",
            names.join(", ")
        );
    }
    Err("nothing would be installed".to_string())
}

/// `--forced-only` answers "what does this mod install if I choose nothing",
/// which is the floor beneath every optional extra and the shape most mods with
/// no installer have anyway.
fn build_selection(bundle: &ModBundle, forced_only: bool) -> Selection {
    if !forced_only {
        return apoc_modengine::default_selection(bundle);
    }
    Selection {
        chosen: bundle
            .options()
            .filter(|o| o.select_mode == apoc_domain::SelectMode::Forced && o.deployable)
            .map(|o| o.id.clone())
            .collect(),
    }
}
