# Contributing to Apocrypha

Thanks for looking. Apocrypha is a Linux-first mod manager, and the things it most needs from contributors are game profiles, mod-format detectors, and bug reports with journals attached.

This document covers the dev setup, the checks your change has to pass, where things live, and what the review will ask of you.

## Development setup

Rust (stable, 1.77 or newer):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

The repository pins the toolchain in `rust-toolchain.toml` (stable, with `rustfmt` and `clippy`), so rustup will fetch the right components on first build.

System libraries for the Tauri shell (Debian family):

```bash
sudo apt install -y build-essential pkg-config \
  libwebkit2gtk-4.1-dev libdbus-1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

Node.js 18 or newer with npm, for the desktop shell.

Clone, build, run:

```bash
git clone https://github.com/Apocrypha-Mods/apocrypha.git
cd apocrypha

cargo build --workspace

cd apps/desktop
npm install
npm run tauri dev
```

If you are only touching Rust, you never need to start the GUI. The core has no GUI dependencies, and `apoc-cli` is enough to drive it:

```bash
cargo run -p apoc-cli -- games
cargo run -p apoc-cli -- game monster-hunter-wilds
cargo run -p apoc-cli -- analyze "/path/to/SomeMod.zip"
```

## Checks your change must pass

Run all of these before opening a pull request. CI runs the same set.

```bash
# Tests (Rust core and Tauri shell)
cargo test --workspace

# Lint, warnings included, tests and examples included
cargo clippy --workspace --all-targets

# Formatting
cargo fmt --all --check

# TypeScript type check
cd apps/desktop && npx tsc --noEmit
```

Clippy must be clean, not "clean apart from a few". If a lint is genuinely wrong for your case, `#[allow(...)]` it at the narrowest possible scope with a comment saying why.

Tests that touch the filesystem use `tempfile` and must not assume anything about the machine they run on: no real Steam install, no network, no `$HOME` writes. Tests that need a Steam layout build a fake one in a temp directory.

## Project layout

```text
apocrypha-desktop/
├─ crates/
│  ├─ apoc-domain/      pure types, zero I/O
│  ├─ apoc-gamedef/     GameDatabaseSource port + LocalBuiltin adapter
│  │  └─ profiles/      declarative game profiles (TOML)
│  ├─ apoc-modengine/   archive -> ModBundle, staging, selection -> DeploymentPlan
│  │  ├─ archive.rs     zip reading, hashing
│  │  ├─ modinfo.rs     Fluffy modinfo.ini parser
│  │  ├─ naming.rs      folder-convention parser
│  │  ├─ normalize.rs   wrapper-strip, installer-model detection
│  │  ├─ rules.rs       format detectors
│  │  ├─ extract.rs     staging extraction (zip-slip guarded)
│  │  └─ plan.rs        selection -> plan, conflicts, layering
│  ├─ apoc-steam/       VDF parser, Steam roots, libraries, compatdata prefixes
│  ├─ apoc-deploy/      place.rs (link ladder), vault.rs, journal.rs, loader.rs
│  ├─ apoc-storage/     SQLite, XDG paths
│  └─ apoc-cli/         developer CLI
└─ apps/desktop/
   ├─ src/              React app
   │  ├─ components/    screens, wizard, dialogs, icons.tsx, ui.tsx
   │  ├─ lib/           Tauri command bindings, state
   │  └─ styles/        theme.css (custom properties), app.css
   └─ src-tauri/        Tauri shell and command surface
```

Dependencies point one way. `apoc-domain` depends on nothing of ours and performs no I/O. Engine crates depend on domain. The UI depends on the Tauri command surface, never on crate internals. If a change makes `apoc-domain` need `std::fs`, the change is in the wrong crate.

## Adding a game profile

Games are data, not code. There is no game-specific branching anywhere in the engine crates, and a pull request that adds some will be asked to move it into the schema.

Add one file:

```text
crates/apoc-gamedef/profiles/<your_game>.toml
```

Use `crates/apoc-gamedef/profiles/monster_hunter_wilds.toml` as the reference. It is commented and covers detection, deploy targets, format list, pak chain and the loader spec including the Proton section.

Then:

1. Register it in the `LocalBuiltin` adapter's bundled list (`crates/apoc-gamedef/src/`).
2. Add a test that loads the profile and asserts the fields that matter (app ID, deploy targets, format order). Every shipped profile has one; a malformed TOML should fail in CI, not on a user's machine.
3. Verify it end to end with `cargo run -p apoc-cli -- game <your-id>`.

If the game needs behaviour the schema cannot express, open an issue before writing code. The right fix is almost always to extend the profile format for everyone rather than to special-case one title.

## Adding a mod-format detector

Detection lives in `detect()` in `crates/apoc-modengine/src/normalize.rs`. It is one ordered chain of checks over the archive's file listing, not a registry of pluggable detectors: an earlier version of this document described a trait selected from a profile's `formats` list, and that has never existed. Build it when a second format needs it, and until then read the chain.

Formats fall into two kinds, and which one you are adding decides where the work goes.

**A format recognised by its shape** — a `natives/` directory, a set of `modinfo.ini` folders, a bare proxy DLL at the root. These are inferred, and they are what the chain is made of.

1. Add a `detect()` arm working from the normalised file listing, never from disk. What counts as a payload root, a rewrapped folder or a loader file comes from `GameRules`, which comes from the game profile: ask the profile rather than writing the game's directory names into the engine.
2. Add a normaliser that turns a matching archive into a `ModBundle`.

**A format that announces itself in a manifest** — FOMOD is the only one so far. These outrank the shape-based chain, because a file the author wrote saying what the archive is beats anything inferred from how it is arranged. They are also gated per game: a manifest is believed only if the profile's `formats` list names the format, since whether a game's mods ship one is a fact about that community rather than about the archive.

Either way:

1. Give it a stable string ID. For a manifest format, add it to `GameRules::supports_format` handling and to the profiles of games that use it. An empty `formats` list means "no opinion", which is read as permission.
2. Add tests with a synthesised archive for the positive case, plus at least one negative case proving it does not steal archives belonging to another format. Detector ordering bugs are the most common failure mode here, so the negative test is not optional. Build the archive in the test rather than checking one in.
3. If you have a real archive that motivated the work, describe its structure in the pull request. Do not commit copyrighted mod archives to the repository.

### What a conditional installer must not do

FOMOD can express things this engine deliberately refuses to guess at. If you extend it, hold to the rule the existing code follows: **anything that changes which files land, with no visible choice attached, either asks the user or refuses the install.** An installer whose conditions do not settle is reported as contradictory rather than iterated over; a destination that escapes the game directory refuses the whole archive rather than skipping one file; and a condition nobody can check stays unknown rather than being assumed either way.

## Code style

**Comments explain WHY, not WHAT.** The code already says what it does. A comment earns its place by recording the reason: the constraint, the bug it avoids, the thing that looks wrong but is deliberate.

```rust
// Steam writes this file on exit, so a running client means our write races
// theirs and loses silently. Refuse rather than corrupt the prefix.
```

Not:

```rust
// Check if steam is running
```

**Every behaviour change ships with a test.** New behaviour gets a test that fails without it. A bug fix gets a test that reproduces the bug first. Refactors keep the existing tests passing untouched; if a refactor requires editing assertions, say why in the pull request, because that usually means behaviour moved too.

**No em dashes in user-facing strings.** Not in the UI, not in CLI output, not in error messages, not in docs. Use commas, colons, parentheses or full stops. This is a hard rule and reviewers will flag it.

**Errors are typed and actionable.** `thiserror` per crate. An error a user can see should say what failed and what they can do about it, not just `Io(std::io::Error)`.

**The safety invariants are not refactorable.** Vault before overwrite, journal before an operation counts as done, hash-guarded deletion on rollback. Any change to `apoc-deploy` that touches ordering around those three will get a slow and detailed review, and it needs tests that kill the process partway through if it changes crash behaviour.

**Frontend.** Plain CSS with custom properties, no Tailwind, no component library. Every colour, size, radius and duration comes from a variable in `theme.css`, because the Appearance panel retheming works by overriding those variables at runtime. Hardcoded hex values will be flagged. Only font weights 400, 500 to 600 and 700 exist, since only three upright faces are available. Icons go in `src/components/icons.tsx`: monoline, `currentColor`, 1.5 stroke on a 24 grid, round caps, no fills. Respect `prefers-reduced-motion`.

## Commits and pull requests

- One logical change per commit. Formatting churn goes in its own commit, not mixed into a fix.
- Present tense, imperative subject line, under 72 characters. `Add 7z archive reader` rather than `added 7z support!!`.
- The body explains why, if that is not obvious from the subject.
- Reference the issue if there is one.
- Pull requests describe what changed and how you verified it. If it touches deployment, say what you ran it against and whether you tested rollback.
- Keep pull requests small enough to review properly. A 2000-line PR gets a worse review than four 500-line ones, which is bad for you as well as for the reviewer.
- Draft PRs are welcome for direction checks before you have finished.

## A note on AI-written code

Some of this project was written with AI assistance, and contributions written that way are fine. The rule is the same for all of it: **AI-written code is untrusted until deterministic checks and human review pass.**

Concretely, that means the tests, clippy, `cargo fmt --check` and `tsc --noEmit` are the evidence, not the model's confidence or yours. Generated tests that assert what the generated code happens to do are worth nothing, so read them and check they assert what the behaviour should be. Plausible-looking code around the vault, the journal or rollback is exactly where a hallucinated ordering will hurt someone's game install, so that code gets read line by line regardless of who or what wrote it.

If you used AI for a non-trivial change, say so in the pull request. Not because it is a problem, but because it tells the reviewer where to look hardest.
