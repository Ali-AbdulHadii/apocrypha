# Roadmap

Phases, not dates. Each one ends at a point where the app is coherent and
usable, so nothing here depends on the next phase landing to be worth having.

Ordering principle: **the things that can lose someone's data or time come
first.** A missing feature is an annoyance. A deployment that half-applies and
cannot be undone is the thing that makes people stop trusting a mod manager.

---

## Phase 1: A mod manager that works, for one game

Status: **done.**

The point of Phase 1 was to prove the hard parts on Linux rather than to be
broad. One game, one loader, one deployment model, taken all the way through.

- Steam and Proton discovery by VDF parsing, including Flatpak and Snap roots
- Mod format detection across the six shapes Wilds mods actually ship in
- Install wizard driven by the mod's own metadata, not a hardcoded option list
- Profiles, load order, conflict detection, dry run
- Journaled deployment with a content-addressed vault and hash-guarded rollback
- REFramework loader setup written into the Proton prefix
- Desktop shell: custom chrome, light and dark, a full token system
- Nexus Mods integration: `nxm://` handler, API client, download queue
- ZIP, 7z and RAR archives, identified by content rather than extension

---

## Phase 2: Trustworthy at scale

Status: **mostly done.** Everything below is shipped except mod updates.

The engine was correct on a handful of mods. This phase was about it staying
correct, and staying legible, when someone has two hundred.

**Deployment progress and cancellation.** Done. Apply runs on its own thread and
emits real file and byte counts, the same pattern the download queue proved.
Cancelling rolls back everything already written, so stopping partway leaves the
game folder as it was rather than half modded. The engine API is
`apply_with(ctx, plan, sink)`, where the sink returns `Flow::Continue` or
`Flow::Cancel`; the old blocking `apply` still exists and delegates to it.

**Verify and repair.** Done. `apoc_deploy::verify` compares the hash the journal
recorded for each deployed file against what is on disk, and `repair` puts back
what it can from staging. Two decisions worth knowing: repair never appends to
the journal, because it only ever writes bytes matching the hash already
recorded, which keeps rollback's guard working; and a file that something else
changed is never repaired unless the user picks it explicitly, because putting it
back discards that change.

**Conflict resolution that is a decision, not a side effect.** Done. A per-file
override pins one contested path to a chosen mod, stored per profile and applied
inside the planner rather than after it, so the winner the interface reports and
the file actually written cannot disagree. A stale override naming a removed mod
is ignored rather than breaking the deploy.

**Large library ergonomics.** Done for the list itself: load order moved to its
own screen so the mod list no longer has to be unfiltered to be reorderable, and
the list windows itself past sixty rows. Row height is measured from a live row
rather than hard-coded, because the spacing and text size are user-settable at
runtime. Saved filters and bulk enable/disable are not done.

**Mod updates.** Not done, and the remaining Phase 2 item. The groundwork is in:
the mods table carries `nexus_mod_id` and `nexus_file_id`, and
`Store::nexus_linked_mods` returns the mods that could be checked. What is
missing is the client call that asks Nexus for the latest file, and a re-deploy
that keeps the options the user already picked.

---

## Phase 3: More games, less hardcoding

Wilds is the only profile that ships, but nothing in the engine is Wilds
specific. The game profile TOML already carries the deploy targets, the pak
chain, the rewrap rules and the loader definition, and that abstraction has not
been tested against a second game.

**Other RE Engine titles first**, because the profile schema was designed from
them: Dragon's Dogma 2, Resident Evil 4 remake, Monster Hunter Rise.

**Then a genuinely different engine**, which is where the profile schema will
break and need to earn its generality. Likely a Bethesda title, since that is
where load order semantics get hard and where the plugin concept appears at all.

**Online game database.** The `GameDatabaseSource` port exists and the local
implementation is complete; the hosted client is not written. Profiles should
arrive without an app update, because a game patch that moves a directory should
not require everyone to wait for a release.

**FOMOD.** XML installers are the Bethesda equivalent of Fluffy's segmented
format, and the wizard's option model was built to be able to represent them.

---

## Phase 4: The Apocrypha service

The name is the argument for this phase. Mods that are hard to find, removed
from Nexus, or circulated in a Discord thread are the ones a mod manager is
least able to help with, and they are the ones people most need help with.

**Apocrypha as a download source.** The switch already exists in Settings and
currently only points at Nexus. The service behind it needs to exist: a catalogue
with permanent links, checksums, and enough metadata for the installer to build
a wizard.

**Mod collections.** A shareable, reproducible set: mods, versions, options,
load order, and the conflict decisions. This is where the journal and the
content hashing pay off, because a collection is only worth sharing if it
installs identically for the person receiving it.

**Preservation.** Content-addressed storage of what has been downloaded, so a
mod that disappears from its host does not disappear from the people who already
had it.

---

## Phase 5: Beyond one machine

**Windows and macOS.** The core is portable Rust; what is Linux specific is
Steam discovery, the Proton prefix, and the link ladder. Those are already
behind boundaries. This is deliberately last, because a Linux-first tool that
gets ported early tends to stop being Linux-first.

**Packaging.** AppImage and `.deb` are the targets. Flatpak is deferred: the
sandbox fights writes into the Steam directory and the Proton prefix, which is
precisely the job.

**Headless mode.** The CLI already drives the core. A profile that can be
applied from a script is what makes a modded install reproducible on a new
machine.

---

## Deliberately not planned

Written down so these stay decisions rather than oversights.

- **A virtual filesystem.** MO2's isolation depends on Windows DLL injection.
  The Linux equivalents (FUSE, `LD_PRELOAD`, OverlayFS) each fail for games run
  through Proton in ways that would make deployment less predictable, not more.
  The vault and journal give reversibility without pretending to isolation.
- **Editing mod contents.** Apocrypha installs mods. A mesh editor is a
  different program.
- **Running the game.** Steam already does this, and a launcher that wraps Steam
  wrapping Proton is a layer that only adds ways to fail.
