//! `apoc plan`, driven as a binary.
//!
//! Tested through the executable rather than by calling the function, because
//! the things worth guaranteeing here are the things a person sees: the exit
//! code, the destinations, and — the reason the command exists — that a mod
//! which would install nothing says so and says why.
//!
//! Archives are synthesized so this runs in CI. `cyberpunk_real_archives.rs` is
//! where the same paths are checked against releases people downloaded.

use std::io::Write;
use std::path::Path;
use std::process::Command;

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

struct Run {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn apoc(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_apoc"))
        .args(args)
        .output()
        .expect("the apoc binary runs");
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        ok: out.status.success(),
    }
}

#[test]
fn it_prints_where_each_file_would_land() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("redscript.zip");
    zip_with(
        &zip,
        &[
            ("engine/config/base/scripts.ini", b"[Scripts]"),
            ("engine/tools/scc.exe", b"COMPILER"),
            ("r6/config/cybercmd/scc.toml", b"[[tasks]]"),
        ],
    );

    let run = apoc(&["plan", "cyberpunk-2077", zip.to_str().unwrap()]);
    assert!(run.ok, "stderr: {}", run.stderr);
    assert!(run
        .stdout
        .contains("Game : Cyberpunk 2077 (cyberpunk-2077)"));
    assert!(run.stdout.contains("3 files"));
    for dest in [
        "{game}/engine/config/base/scripts.ini",
        "{game}/engine/tools/scc.exe",
        "{game}/r6/config/cybercmd/scc.toml",
    ] {
        assert!(
            run.stdout.contains(dest),
            "missing {dest} in:\n{}",
            run.stdout
        );
    }

    // The command must be unable to leave anything behind, and must say so
    // rather than leaving the reader to wonder whether it installed the mod.
    assert!(run.stdout.contains("Nothing was installed"));
}

#[test]
fn a_mod_that_would_install_nothing_says_what_it_has_and_what_the_game_wants() {
    // The branch the command is worth building for. Without it, "the mod
    // installed and did nothing" is guesswork; with it, both halves of the
    // mismatch are on screen.
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("SkyrimMod.zip");
    zip_with(
        &zip,
        &[
            ("Data/Meshes/armor.nif", b"MESH"),
            ("Data/Textures/armor.dds", b"TEX"),
        ],
    );

    let run = apoc(&["plan", "cyberpunk-2077", zip.to_str().unwrap()]);
    assert!(
        !run.ok,
        "installing nothing is a failure, not a quiet success"
    );
    assert!(run.stdout.contains("Nothing to install"));
    assert!(
        run.stdout.contains("Archive roots seen: Data"),
        "the archive's own roots must be named:\n{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("archive, mods, r6, red4ext, bin, engine"),
        "the game's payload roots must be named:\n{}",
        run.stdout
    );
}

#[test]
fn the_same_archive_answers_differently_for_two_games() {
    // This is the defect `plan` exists to make impossible. `analyze` assumes RE
    // Engine, so it reports a Cyberpunk mod as empty; naming the game is the
    // difference between four files and none.
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("mod.zip");
    zip_with(
        &zip,
        &[("r6/scripts/CarDealer/Main.reds", b"module CarDealer")],
    );

    let cp = apoc(&["plan", "cyberpunk-2077", zip.to_str().unwrap()]);
    assert!(cp.ok, "stderr: {}", cp.stderr);
    assert!(cp.stdout.contains("{game}/r6/scripts/CarDealer/Main.reds"));

    let wilds = apoc(&["plan", "monster-hunter-wilds", zip.to_str().unwrap()]);
    assert!(!wilds.ok);
    assert!(wilds.stdout.contains("Nothing to install"));
    assert!(wilds
        .stdout
        .contains("Payload roots for monster-hunter-wilds"));
}

#[test]
fn analyze_warns_when_no_game_was_named() {
    // It still answers with RE Engine's rules, because that is what it has
    // always done and changing it would break every existing caller. What it
    // must not do any more is answer as though the question were unambiguous.
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("mod.zip");
    zip_with(&zip, &[("r6/scripts/Mod/Main.reds", b"module Mod")]);

    let bare = apoc(&["analyze", zip.to_str().unwrap()]);
    assert!(bare.ok);
    assert!(
        bare.stderr.contains("no game given"),
        "an answer computed with the wrong game must be labelled: {}",
        bare.stderr
    );

    let named = apoc(&["analyze", zip.to_str().unwrap(), "cyberpunk-2077"]);
    assert!(named.ok, "stderr: {}", named.stderr);
    assert!(named.stdout.contains("Rules  : Cyberpunk 2077"));
    assert!(
        !named.stderr.contains("no game given"),
        "naming the game must silence the warning"
    );
    assert!(
        named.stdout.contains("1 deployable"),
        "with the right rules the mod is not empty:\n{}",
        named.stdout
    );
}

#[test]
fn an_unknown_game_fails_instead_of_guessing() {
    let tmp = tempfile::tempdir().unwrap();
    let zip = tmp.path().join("mod.zip");
    zip_with(&zip, &[("r6/scripts/Mod/Main.reds", b"module Mod")]);

    let run = apoc(&["plan", "half-life-3", zip.to_str().unwrap()]);
    assert!(!run.ok);
    assert!(run.stderr.contains("error:"));
}

#[test]
fn missing_arguments_print_the_usage_rather_than_a_panic() {
    let run = apoc(&["plan"]);
    assert!(!run.ok);
    assert!(run
        .stderr
        .contains("usage: apoc plan <game-id> <archive.zip>"));

    let run = apoc(&["plan", "cyberpunk-2077"]);
    assert!(!run.ok);
    assert!(run.stderr.contains("usage: apoc plan"));
}
