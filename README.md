<img src="assets/logo-white.png#gh-dark-mode-only" alt="Apocrypha" width="88">
<img src="assets/logo-black.png#gh-light-mode-only" alt="Apocrypha" width="88">

# Apocrypha

A native Linux desktop mod manager for games, built Linux-first rather than ported to it. Ships profiles for **Monster Hunter Wilds**, **Cyberpunk 2077**, **Skyrim Special Edition**, **Dragon's Dogma 2**, **Resident Evil 4** and **Monster Hunter Rise**.

Linux game modding is badly served. The established managers are Windows programs: Vortex needs a Wine prefix and a lot of goodwill to behave, Fluffy Mod Manager is a Windows GUI that people run through Proton and then fight over paths, and Mod Organizer 2 cannot be fixed by porting effort at all. MO2's whole design rests on USVFS, a user-space virtual filesystem built from Windows DLL injection and API hooking. There is no Linux equivalent to hook, so the feature that makes MO2 good is the feature that cannot cross over. That leaves Linux players doing the thing everyone eventually does: copying files into the game directory by hand, keeping a text file of what they changed, and hoping they can undo it later.

Apocrypha starts from that reality instead of working around it. It is a Rust core plus a Tauri desktop shell that understands Steam and Proton on Linux natively, keeps every staged file outside the game directory, and treats deployment as a journaled transaction that can be rolled back file by file. Where Windows tools reach for a virtual filesystem, Apocrypha reaches for reflinks, hardlinks, a content-addressed vault of displaced originals, and an append-only log of every operation it performed.

MIT licence. Linux (x86_64), Steam and Proton.

## Status

**Early development.** Version 0.2, Phase 3. Six game profiles ship today, and the deployment engine has been exercised end to end against real segmented installers but not yet against a wide spread of community mods. Expect rough edges, expect the UI to change, and keep a backup of anything you cannot re-download. The safety machinery (vault, journal, hash-guarded rollback) is the part that has had the most attention, because it is the part that can ruin your day if it is wrong.

Phase 2 added the things a library needs once it stops being small: applying is interruptible and reports real progress, the game folder can be checked against the change log and repaired, load order has its own screen with per-file overrides, and the mod list windows itself so a few hundred mods stay smooth.

Known gaps today:

| Gap | Detail |
| --- | --- |
| Archive formats | ZIP, 7z and RAR. Encrypted and multi-volume archives are not handled. |
| Plugin sorting | The plugin list is managed and editable, but Apocrypha has no opinion about what a good order is. There is no LOOT-style automatic sort: it tells you when a plugin loads before something it depends on and offers the move that fixes it, and the arranging is yours. |
| Loader binary | Apocrypha configures REFramework for Proton but does not redistribute `dinput8.dll`. You supply it. SKSE installs correctly but nothing reports whether it is set up, because the profile schema has no launcher concept yet — only "no loader" and "DLL proxy". |
| Platform | Linux is the supported target and the only one that ships a build. The engine compiles for Windows, finds Steam games there, and registers `nxm://` in the registry, but nothing else has been tested and there is no installer. macOS is untouched. |

## Features

What works today:

- **Steam and Proton discovery.** VDF parsing for `libraryfolders.vdf`, `appmanifest_*.acf` and `config.vdf`. Finds native, Flatpak and Snap Steam roots, resolves library folders, install directories, the active Proton build, and the game's `compatdata` prefix.
- **Mod format detection.** Fluffy segmented "AIO" installers (one `modinfo.ini` per option), single Fluffy mods, flat `natives/` dumps, `reframework/` script and plugin mods, bare loader DLLs such as `dinput8.dll`, standalone `.pak` mods renamed into the RE Engine patch chain, and `pak_mods/` directory mods.
- **FOMOD installers.** The conditional format most of the Bethesda ecosystem ships in: install steps, groups with real cardinality, plugin types, condition flags, steps that appear because of an earlier answer, and per-file destinations and priorities. A manifest outranks anything inferred from an archive's shape, and where a manifest asks for something that cannot be honoured exactly, the installer says so or refuses rather than guessing at which files you wanted.
- **Mods that install beside the game executable.** Script extenders, ENB, ReShade, DXVK — the whole class that does not live under `Data` or `natives`. Both packagings are understood with no repackaging: a `Root` folder mirroring the game directory, which is the convention Mod Organizer's Root Builder established, and loose files at the archive root, which is how SKSE itself ships. Which files a game accepts there is declared by its profile as patterns, so a library whose name carries the game version still matches after an update, and a readme sitting beside it still does not. They arrive as their own already-selected option, so you can see exactly what is going next to the game binary and decline it without declining the mod. No modes to choose, nothing to run before or after launching, and no repackaging: the vault, the change log and hash-guarded rollback cover these files exactly as they cover everything else.
- **Install wizard for segmented installers.** Options, roles and radio sets are derived from the mod's own payload and metadata, not from a hardcoded list. Independent choices stay independent instead of collapsing into one giant radio group.
- **Profiles.** Separate selections per profile, so a "clean run" profile and a "everything on" profile can coexist over the same installed mods.
- **Enable and disable without deleting.** Staged payloads live in Apocrypha's own directory. Turning a mod off removes its deployed files, not your download.
- **Load order on its own screen.** A flat, always-draggable list with keyboard move controls, showing how many contested files each mod wins and loses. Mods stays a library you browse; ordering is its own job.
- **Plugin order on its own screen**, for Creation Engine games. A game's plugin list is a named list of files rather than a priority per mod, so it gets its own screen: drag or use the arrows, switch a plugin on or off, and see the master block the engine loads first drawn as the boundary it is. What a plugin depends on is read out of the file's own header, so it can tell you when one loads before something it needs and offer the single move that fixes it. Every change is written to the game's list straight away — vaulted and journaled first, like everything else that writes.
- **Conflict detection, and per-file overrides.** Before anything is written, you get the list of files two mods both want and which one wins. You can pin a single file to a different mod without reordering anything, so you can take one mod's mesh and another's texture.
- **Dry run preview.** The full plan (every file, every destination, the placement method chosen) without touching the game directory.
- **Journaled deployment with hash-guarded rollback.** Every operation is flushed to an append-only JSONL journal as it happens. Undo replays in reverse and refuses to delete a file whose bytes changed since deploy.
- **Interruptible apply with real progress.** Deployment runs off the main thread and reports files and bytes as it writes. Stopping partway rolls back what it already wrote, so a cancelled apply leaves the game folder exactly as it was.
- **Verify and repair.** Compares the change log against what is actually in the game folder and reports anything missing or altered. Missing files can be put back from staging; altered files are left alone unless you say otherwise, because putting one back discards whatever changed it.
- **A mod list that stays smooth.** The list windows itself past sixty rows, measuring row height from a live row so it keeps working when you change the spacing or text size.
- **Honest disk usage.** Settings shows the measured size of the mod library, the backups, the change log and the downloads folder, with a way into each. Managers quietly consume tens of gigabytes and rarely say so.
- **REFramework loader setup for Proton.** Writes the `dinput8=n,b` DLL override into the prefix's `user.reg` atomically, captures the previous value for rollback, and refuses to touch the prefix while Steam is running.
- **Download links from the website.** Apocrypha can take over the `nxm://` scheme so the Mod Manager Download button sends files straight here — through a desktop entry on Linux, through the per-user registry on Windows. If another manager already holds it, you are told which one and asked before it changes hands, and turning it off gives the scheme back rather than leaving your downloads handled by nothing.
- **Downloads.** Nexus `nxm://` links download in the background with live progress, and wait on a Downloads screen until you choose to install them. The folder is configurable, and anything already in it is listed and installable, so archives saved from a browser or brought from another manager work the same way. Rows show which files are already in your library.
- **Light and dark themes.** Every colour, size and radius is a CSS custom property, so the Appearance panel can retheme the whole app at runtime.

## The screens

| Screen | What it is for |
| --- | --- |
| Library | Detected games, the Steam root and Proton prefix found for each, and the mod loader setup. |
| Mods | The library you browse: search, filter by state or category, enable, disable, reconfigure, remove. |
| Load order | The sequence you arrange. Flat, always draggable, keyboard reachable, and it shows which contested files each mod wins and loses. |
| Downloads | Transfers in progress, files ready to install, and anything already sitting in your downloads folder. |
| Profiles | Independent sets of mods, options and order over the same installed library. |
| Changes | The dry run before you apply, and the check that your game folder still matches what was written. |
| Settings | Appearance, downloads and the Nexus account, where things live on disk, and the developer options. |

## Install

### Download a build

The [releases page](https://github.com/Apocrypha-Mods/apocrypha/releases) has an
AppImage and a `.deb` for x86_64 Linux.

**AppImage**, which runs anywhere without installing:

```bash
chmod +x Apocrypha_*_amd64.AppImage
./Apocrypha_*_amd64.AppImage
```

**Debian, Ubuntu, Pop!_OS, Mint:**

```bash
sudo apt install ./Apocrypha_*_amd64.deb
```

The `.deb` registers Apocrypha as the handler for `nxm://` links, so the Mod
Manager Download button on Nexus Mods sends files straight to the app. With the
AppImage, use the button in Settings under Downloads to register it yourself.

### Or build from source

### Prerequisites

Rust (stable toolchain, 1.93 or newer):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

Node.js 18 or newer, and npm. Use your distribution's package or a version manager.

System libraries for the Tauri shell (Debian, Ubuntu, Pop!_OS, Mint):

```bash
sudo apt install -y build-essential pkg-config \
  libwebkit2gtk-4.1-dev libdbus-1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

On Fedora the equivalents are `webkit2gtk4.1-devel`, `gtk3-devel`, `libappindicator-gtk3-devel`, `librsvg2-devel`, `libxdo-devel`, `openssl-devel`. On Arch: `webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator`, `librsvg`, `xdotool`, `openssl`.

### Build and run

Rust and Node can live inside the checkout rather than on the machine, so a
build does not depend on what the system happens to have. Source the activation
script for your shell first — `env.fish` for fish, `env.sh` for bash and zsh.
Sourcing the wrong one fails on its first line, because `export VAR=value` is a
syntax error in fish and the resulting message reads like a broken toolchain
rather than a wrong file:

```bash
source env.sh      # bash, zsh
```

```fish
source env.fish    # fish
```

Skip this if Rust and Node are already installed system-wide; the commands below
work either way.

```bash
git clone https://github.com/Apocrypha-Mods/apocrypha.git
cd apocrypha

# Rust core: build and test
cargo test --workspace

# Desktop app: install JS dependencies, then run
cd apps/desktop
npm install
npm run tauri dev
```

For a release build:

```bash
cd apps/desktop
APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 npm run tauri build
```

Both variables work around `linuxdeploy`, which builds the AppImage. It ships as
an AppImage itself and mounts through FUSE 2, so on a distribution that carries
only `fuse3` it cannot start at all; `APPIMAGE_EXTRACT_AND_RUN=1` makes it
unpack and run instead of mounting. `NO_STRIP=1` skips its pass over the
bundled GTK and WebKit libraries, which fails on some toolchains and takes the
AppImage down with it. The application binary is stripped either way — the
release profile in `Cargo.toml` sets `strip = true` — so the cost is symbols
left in the bundled system libraries and roughly 20 MB of AppImage. Plain
`npm run tauri build` is correct wherever `linuxdeploy` runs.

Packaging targets are AppImage (primary) and `.deb`. Flatpak is deferred: the sandbox fights writes into the Steam directory and the Proton prefix, which is exactly what a mod manager has to do.

### Developer CLI

The core has no GUI dependencies, so you can drive it from a terminal:

```bash
cargo run -p apoc-cli -- games
cargo run -p apoc-cli -- game monster-hunter-wilds
cargo run -p apoc-cli -- analyze "/path/to/SomeMod.zip"
```

`analyze` is the fastest way to see how Apocrypha reads an archive: detected format, options, radio sets, deploy roots.

## Usage: your first mod

1. **Detect the game.** Open the Library screen. Apocrypha scans your Steam roots (native, Flatpak, Snap), matches the app ID from the game profile, and shows the install directory and the Proton prefix it found. If detection misses, you can point it at the directory yourself.
2. **Add a mod.** Drag an archive onto the Mods screen or use Add Mod. ZIP, 7z and RAR all work, and the format is read from the file's leading bytes rather than its extension, so a mislabelled archive still opens. The archive is hashed, read, and normalised into a canonical bundle. Nothing is written to the game directory at this stage.
3. **Choose options.** If the archive is a segmented installer, the wizard opens with the options it found. Radio sets are clustered by their real relationships, so picking a body physics variant does not deselect an unrelated leg physics variant. Single-payload mods skip the wizard.
4. **Enable it.** The mod appears in the list with a switch. Enabling adds it to the current profile's selection and computes its place in the load order. Still nothing written.
5. **Preview, then apply.** The apply bar shows a dry run first: every source file, every destination, the placement method (reflink, hardlink, symlink or copy) and any conflicts with mods already deployed. If it looks right, apply. Displaced originals go to the vault before they are replaced, and each operation is appended to a journal as it completes.
6. **Set up the loader.** Most games need one, and which one comes from the game profile. For Monster Hunter Wilds it is REFramework; for Cyberpunk 2077 it is RED4ext, whose proxy lives at `bin/x64/winmm.dll` and which registers two overrides, because Cyber Engine Tweaks is a second proxy in the same folder. Taking Wilds as the example, loose files only load through REFramework. Put `dinput8.dll` where Apocrypha asks for it, then use the loader panel: it writes the `dinput8=n,b` override into the Proton prefix's `user.reg` and shows you the equivalent Steam launch option (`WINEDLLOVERRIDES="dinput8=n,b" %command%`) if you prefer to set it there. Close Steam first; Apocrypha will refuse while it is running.
7. **Undo.** Every deployment has a journal entry. Undo replays it in reverse: deployed files removed, vaulted originals restored, empty directories pruned, the previous `user.reg` value put back. Any file whose hash no longer matches what was deployed is left in place and reported rather than deleted, because a changed hash means something other than Apocrypha wrote to it.

## How it works

The core is a Cargo workspace of small crates with one direction of dependency: pure types at the bottom, I/O at the edges, the UI as a replaceable shell on top.

| Crate | Responsibility |
| --- | --- |
| `apoc-domain` | Pure types, zero I/O: `GameProfile`, `ModBundle`, `ModOption`, `Selection`, `DeploymentPlan`, `PakChainSpec`, `DeployRoot`, `SelectMode`. |
| `apoc-gamedef` | `GameDatabaseSource` port plus a `LocalBuiltin` adapter that reads bundled TOML game profiles. |
| `apoc-modengine` | Archive to canonical `ModBundle`: Fluffy `modinfo.ini` parser, folder-naming parser, staging extraction (zip-slip guarded), selection to `DeploymentPlan` with conflict resolution. |
| `apoc-steam` | Steam and Proton discovery: VDF parser, native/Flatpak/Snap roots, `libraryfolders.vdf`, `appmanifest`, `compatdata` prefixes. |
| `apoc-deploy` | The deployment engine: adaptive link ladder, content-addressed vault, append-only JSONL journal, hash-guarded rollback, REFramework Proton loader provisioning. |
| `apoc-storage` | SQLite state (bundled `rusqlite`, WAL mode), XDG paths, games, mods, profiles, selections, deployments. |
| `apoc-cli` | The `apoc` developer CLI. |
| `apps/desktop` | Tauri v2 shell, React 18 and TypeScript UI, plain CSS with custom properties. |

### The placement ladder

Files are placed by trying, in order: reflink (`FICLONE`, on Btrfs and XFS), hardlink (same filesystem, the usual case on ext4), symlink, then a plain copy. The engine probes the pair of directories once and uses the best method that actually works there, so a staged mod normally costs no extra disk space.

### Three safety invariants

Everything else in the deploy engine is negotiable. These are not.

1. **Vault before overwrite.** No pre-existing game file is ever replaced until its bytes have been copied into the content-addressed vault. If the process is killed between the two steps, the original still exists.
2. **Journal before it counts.** An operation is only considered done once its record is flushed to the append-only journal. A crash can leave work that the journal does not know about, which is recoverable, but never work the journal knows about that did not happen.
3. **Hash-guarded deletion.** Rollback hashes a file before removing it and compares against what was recorded at deploy time. A mismatch means something else wrote there (a game update, a manual edit, another tool), so the file is left alone and reported. Apocrypha would rather leave a mod file behind than delete something it did not put there.

### On-disk layout

Staging, the vault and journals live outside the game directory, so the game install stays disposable and safe to run Steam's "verify integrity of game files" against.

```text
$XDG_DATA_HOME/apocrypha/          (default ~/.local/share/apocrypha)
├─ apocrypha.db                    SQLite: games, mods, profiles, selections, deployments
└─ games/<game-id>/
    ├─ staging/<mod-id>/           extracted payloads, namespaced per option
    ├─ vault/<aa>/<hash...>        original game files displaced by a deploy
    └─ journal/<deployment-id>.jsonl
                                   append-only operation log, one JSON object per line
```

## Supported games

| Game | ID | Engine | Status |
| --- | --- | --- | --- |
| Monster Hunter Wilds | `monster-hunter-wilds` | RE Engine | Primary target |
| Cyberpunk 2077 | `cyberpunk-2077` | REDengine | Archive, REDmod, redscript, TweakXL, RED4ext and CET layouts |
| Dragon's Dogma 2 | `dragons-dogma-2` | RE Engine | REFramework, Fluffy layouts and standalone paks |
| Resident Evil 4 (2023) | `resident-evil-4-2023` | RE Engine | REFramework, Fluffy layouts and standalone paks |
| Monster Hunter Rise | `monster-hunter-rise` | RE Engine | REFramework, Fluffy layouts and standalone paks |

### Adding a game

**Games are data, not code.** No engine crate contains `if game == "..."`. A game is one declarative TOML document, and adding one means adding that document:

```
crates/apoc-gamedef/profiles/<your_game>.toml
```

> **One TOML trap worth knowing.** A bare key belongs to the most recent
> `[section]`, so `canonical_case = [...]` written below `[loader.proton]`
> parses cleanly and is then silently ignored. Keep every top-level key above
> the first section. The leaf structs set `deny_unknown_fields`, so a misplaced
> key now fails to parse instead of doing nothing.

The profile declares the app ID and executable used for detection, the payload roots and where they map to inside the game directory, the load-order and conflict policies, and the loader specification if the game needs one. Abridged, that looks like:

`formats` documents which archive shapes the game's mods actually come in. It is descriptive today: detection derives what it needs from the payload roots, the rewrap rules and the loader, so the list records intent rather than driving behaviour.

```toml
id = "monster-hunter-wilds"
name = "Monster Hunter Wilds"
engine = "re-engine"
load_order = "priority"
conflict_scope = "per-relative-path"
case_sensitive = true

[detection]
steam_app_id = 2246340
executable = "MonsterHunterWilds.exe"

[[deploy_targets]]
source = "natives"
target = "natives"

formats = ["fluffy-aio", "fluffy-single", "loose-natives", "reframework-only", "pak"]

[pak_chain]
pattern = "re_chunk_000.pak.sub_000.pak.patch_{n}.pak"
digits = 3
start_index = 1

[loader]
name = "REFramework"
kind = "dll-proxy"
proxy_dll = "dinput8.dll"

[loader.proton]
wine_dll_overrides = "dinput8=n,b"
steam_launch_options = 'WINEDLLOVERRIDES="dinput8=n,b" %command%'
requires_prefix_write = true
```

If your game needs a behaviour the schema cannot express, that is a gap in the schema and worth an issue. The intended failure mode is "extend the profile format", not "add a branch to the engine".

## Roadmap

Phases, not dates. Phase 1 is done: a mod manager that works end to end for one
game, with the safety machinery built first.

- **Phase 2, trustworthy at scale.** Mostly done. Apply progress and
  cancellation, verify-and-repair against the journal, per-file conflict
  overrides, a dedicated load order screen, and a mod list that stays usable at
  two hundred mods have all landed. Mod updates from Nexus are the remaining
  piece: the database records which Nexus mod and file each install came from,
  but nothing checks for a newer one yet.
- **Phase 3, more games.** Other RE Engine titles first, then an engine
  different enough to test whether the game profile schema is really general.
  Online game database and FOMOD support land here.
- **Phase 4, the Apocrypha service.** Apocrypha as a download source, shareable
  reproducible collections, and preservation of mods that vanish from their host.
- **Phase 5, beyond one machine.** Windows and macOS, packaging, headless apply.

Each phase ends somewhere the app is coherent and usable, and the ordering
principle is that the things which can lose someone's data or time come first.

## Contributing

Bug reports, game profiles and format detectors are all welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the dev setup, the test and lint commands, the project layout, and the code style expectations.

If you are filing a bug about a deployment going wrong, please include the journal file from `~/.local/share/apocrypha/games/<game-id>/journal/`. It is a plain JSONL log of exactly what was done and it is the single most useful thing you can attach.

## Licence

MIT. See [LICENSE](LICENSE).

RAR support is provided by the `unrar` crate, which builds RARLAB's UnRAR
sources. Those carry their own licence: free to use for extracting RAR archives,
but the source may not be used to develop a compatible RAR compressor. Apocrypha
only ever reads RAR files, which is within those terms, but the condition is
worth knowing if you redistribute a build. 7z support is `sevenz-rust2`, which
is pure Rust under the usual permissive terms.

## Acknowledgements

- **REFramework** by praydog, and everyone who has worked on it. It is the reason loose-file modding on RE Engine is possible at all, on any platform.
- **Fluffy Mod Manager**, whose `modinfo.ini` convention became the de facto packaging format for RE Engine mods. Apocrypha reads it because the community already writes it.
- **Mod Organizer 2**, as prior art. Profiles, explicit load order, keeping mods out of the game directory and never destroying the original install are its ideas. Apocrypha's job is to reach the same guarantees on a platform where USVFS cannot follow.
