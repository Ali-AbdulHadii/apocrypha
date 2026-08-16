//! `apoc`: a thin developer CLI over the Apocrypha core.
//!
//! It is NOT the product (that is the Tauri desktop app). It exists so the core
//! engines can be exercised and demonstrated end-to-end before the UI lands.
//!
//! Usage:
//!   apoc games                 List builtin game profiles.
//!   apoc game <id>             Show one game profile.
//!   apoc analyze <archive>     Parse a mod archive and print its wizard tree.
//!   apoc plan <game> <archive> Show where the mod's files would land.

use apoc_domain::{ModBundle, SelectMode};
use apoc_gamedef::{GameDatabaseSource, LocalBuiltin};
use std::path::Path;
use std::process::ExitCode;

mod plan_command;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");

    let result = match cmd {
        "games" => cmd_games(),
        "game" => match args.get(1) {
            Some(id) => cmd_game(id),
            None => Err("usage: apoc game <id>".to_string()),
        },
        // The game id is optional here only because omitting it is what this
        // command has always done. Naming one is the correct way to use it, and
        // `plan` is the command that makes naming one compulsory.
        "analyze" => match args.get(1) {
            Some(p) => cmd_analyze(Path::new(p), args.get(2).map(String::as_str)),
            None => Err("usage: apoc analyze <archive.zip> [game-id]".to_string()),
        },
        "plan" => {
            let forced_only = args.iter().any(|a| a == "--forced-only");
            let positional: Vec<&String> =
                args[1..].iter().filter(|a| !a.starts_with("--")).collect();
            match positional.as_slice() {
                [game, archive] => {
                    plan_command::run(game, Path::new(archive.as_str()), forced_only)
                }
                _ => Err("usage: apoc plan <game-id> <archive.zip> [--forced-only]".to_string()),
            }
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command '{other}'. Try `apoc help`.")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("apoc: Apocrypha core CLI (developer tool)\n");
    println!("  apoc games              List builtin game profiles");
    println!("  apoc game <id>          Show one game profile");
    println!("  apoc analyze <archive> [game-id]");
    println!("                          Parse a mod archive into its wizard tree");
    println!("  apoc plan <game-id> <archive> [--forced-only]");
    println!("                          Show where that mod's files would land");
}

fn cmd_games() -> Result<(), String> {
    let src = LocalBuiltin::new();
    let games = src.all().map_err(|e| e.to_string())?;
    println!("Builtin game profiles ({}):", games.len());
    for g in games {
        println!(
            "  {:<24} {}  [appid {}, engine {:?}, load-order {:?}]",
            g.id, g.name, g.detection.steam_app_id, g.engine, g.load_order
        );
    }
    Ok(())
}

fn cmd_game(id: &str) -> Result<(), String> {
    let src = LocalBuiltin::new();
    let g = src.get(id).map_err(|e| e.to_string())?;
    println!("{} ({})", g.name, g.id);
    println!("  engine        : {:?}", g.engine);
    println!("  steam app id  : {}", g.detection.steam_app_id);
    println!("  load order    : {:?}", g.load_order);
    println!("  case-sensitive: {}", g.case_sensitive);
    println!("  deploy targets:");
    for t in &g.deploy_targets {
        println!("      {} -> {{game}}/{}", t.source, t.target);
    }
    if let Some(l) = &g.loader {
        println!("  loader        : {} ({:?})", l.name, l.kind);
        if let Some(dll) = &l.proxy_dll {
            println!("      proxy dll : {{game}}/{dll} (always a real copy, never linked)");
        }
        for (name, value) in l.dll_overrides() {
            println!("      override  : {name}={value}");
        }
        println!("      prefix    : write={}", l.proton.requires_prefix_write);
    }
    Ok(())
}

/// Parse an archive and print its wizard tree.
///
/// Without a game id this uses RE Engine's rules, which is what it has always
/// done and is wrong for every other game: run against a Cyberpunk mod it
/// reports nothing deployable, because none of `archive/`, `r6/` or `red4ext/`
/// is a payload root for Monster Hunter. The id is accepted so the answer can
/// be right, and the warning is printed when it is absent so a wrong answer is
/// at least a labelled one.
fn cmd_analyze(path: &Path, game_id: Option<&str>) -> Result<(), String> {
    let bundle = match game_id {
        Some(id) => {
            let profile = LocalBuiltin::new().get(id).map_err(|e| e.to_string())?;
            let rules = apoc_modengine::GameRules::from_profile(&profile);
            println!("Rules  : {} ({})\n", profile.name, profile.id);
            apoc_modengine::analyze_archive_with(path, &rules).map_err(|e| e.to_string())?
        }
        None => {
            eprintln!(
                "note: no game given, so RE Engine rules are assumed. \
                 For any other game run `apoc analyze <archive> <game-id>`."
            );
            apoc_modengine::analyze_archive(path).map_err(|e| e.to_string())?
        }
    };
    print_bundle(&bundle);
    Ok(())
}

/// How an option reads at a glance: whether it starts selected, and its role.
///
/// `recommended` is part of the answer, not decoration. An add-on that is in the
/// default selection was rendering as an empty box, which said the opposite of
/// what installing the mod would actually do.
fn role_glyph(mode: SelectMode, recommended: bool) -> &'static str {
    match mode {
        SelectMode::Forced => "[x] forced   ",
        SelectMode::Exclusive if recommended => "(o) one-of   ",
        SelectMode::Exclusive => "( ) one-of   ",
        SelectMode::Stackable if recommended => "[x] addon    ",
        SelectMode::Stackable => "[ ] addon    ",
        SelectMode::Info => " i  info     ",
    }
}

pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

fn print_bundle(b: &ModBundle) {
    println!("Bundle : {}", b.name);
    println!(
        "         {}  by {}  [{:?}]",
        b.version.as_deref().unwrap_or("v?"),
        b.author.as_deref().unwrap_or("unknown"),
        b.installer_model
    );
    if let Some(h) = &b.archive_sha256 {
        println!("         sha256 {}…{}", &h[..8], &h[h.len() - 8..]);
    }
    let total: u64 = b.deployable_options().map(|o| o.total_size()).sum();
    println!(
        "         {} groups, {} options, {} deployable, {} total payload\n",
        b.groups.len(),
        b.option_count(),
        b.deployable_options().count(),
        human_size(total)
    );

    for g in &b.groups {
        let idx = g.index.map(|i| i.to_string()).unwrap_or_else(|| "-".into());
        let radio_sets = g.radio_sets();
        let kind = match radio_sets.len() {
            0 => "optional".to_string(),
            1 => "pick one".to_string(),
            n => format!("{n} independent choices"),
        };
        println!("â”€â”€ [{idx}] {} ({kind})", g.label);

        // Non-exclusive options first (forced / addon / info), in order.
        for o in g.options.iter().filter(|o| o.radio_set.is_none()) {
            println!(
                "     {}{:<40} {}",
                role_glyph(o.select_mode, o.recommended),
                truncate(&o.name, 40),
                files_label(o)
            );
        }
        // Then each radio set under its own "pick one of" sub-header.
        for (i, key) in radio_sets.iter().enumerate() {
            let members: Vec<_> = g
                .options
                .iter()
                .filter(|o| o.radio_set.as_ref() == Some(key))
                .collect();
            let sub = key.split_once(':').map(|x| x.1).unwrap_or(key);
            println!("     â”Œ pick one of {} ({} options):", sub, members.len());
            for o in members {
                println!(
                    "     â”‚ {}{:<38} {}",
                    role_glyph(o.select_mode, o.recommended),
                    truncate(&o.name, 38),
                    files_label(o)
                );
            }
            if i + 1 == radio_sets.len() {
                // no-op; keep output compact
            }
        }
    }
}

fn files_label(o: &apoc_domain::ModOption) -> String {
    if o.deployable {
        format!("{} files, {}", o.payload.len(), human_size(o.total_size()))
    } else {
        "notice".to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
