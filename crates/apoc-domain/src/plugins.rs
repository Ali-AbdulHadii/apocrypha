//! Creation Engine plugin order: the list a game reads, as a domain type.
//!
//! Every other ordering in Apocrypha is an integer per mod. This one is not. A
//! Bethesda game reads a *named list of files* whose position is meaningful, and
//! the list is independent of which mod supplied each entry: two mods can ship
//! one plugin each and the user can want them in either order, while a third mod
//! ships no plugin at all and is still ordered by priority like everything else.
//!
//! Two constraints come from the files rather than from anybody's preference,
//! and they are what makes this a type rather than a `Vec<String>`:
//!
//! **Masters load first.** A plugin names the plugins it depends on in its own
//! header, and loading it before one of them is not a preference the user is
//! entitled to — the game will not start, or will start with the mod's records
//! silently absent. [`PluginOrder::violations`] finds those; nothing here
//! reorders to fix them, because a plugin order is something a person curated
//! and moving their entries is a decision they did not ask for.
//!
//! **Masters are also a sort class.** Creation Engine loads every `.esm` before
//! any `.esp`, whatever the list says, and light plugins (`.esl`, or the light
//! flag) are indexed separately. A list that ignores this describes an order the
//! game will not use, so [`PluginKind`] is carried and honoured.
//!
//! Pure, like everything else here: parsing a plugin's header is I/O and lives
//! in `apoc-modengine`, writing the list is I/O and lives in `apoc-deploy`.

use serde::{Deserialize, Serialize};

/// What kind of plugin a file is, which decides the class it sorts into.
///
/// Read from the file's own header rather than its extension. A `.esp` carrying
/// the master flag is a master, and mods ship them: the extension says what the
/// author called it, the flag says what the engine does with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginKind {
    /// A master. Loads before every non-master, whatever the list says.
    Master,
    /// A light plugin: loads in the master block but takes an index in a
    /// separate, larger space, which is the entire reason it exists.
    LightMaster,
    /// An ordinary plugin.
    Normal,
}

impl PluginKind {
    /// Whether the engine loads this in the master block.
    pub fn loads_as_master(self) -> bool {
        matches!(self, PluginKind::Master | PluginKind::LightMaster)
    }
}

/// One entry in a game's plugin list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEntry {
    /// The file name as the game reads it, e.g. `MyMod.esp`. Never a path: the
    /// list names files in one directory and nothing else is addressable.
    pub name: String,
    /// Whether the game loads it. In `plugins.txt` this is the `*` prefix.
    pub enabled: bool,
    pub kind: PluginKind,
    /// The plugins this one names in its header, in header order.
    ///
    /// Empty for a plugin whose header could not be read, which is treated as
    /// "no constraint known" rather than "no constraints": a plugin is never
    /// refused for being unreadable, it is simply not defended.
    pub masters: Vec<String>,
    /// Whether the game loads this whether or not the list mentions it.
    ///
    /// `Skyrim.esm` and its official add-ons are loaded by the engine itself.
    /// They are marked rather than removed, because a list that omits them is
    /// one where every other entry's masters look unsatisfied.
    pub implicit: bool,
}

impl PluginEntry {
    /// A plugin about which nothing is known but its name.
    ///
    /// The kind falls back to the extension, which is right for the common case
    /// and wrong only for a flag that contradicts it — and a file we could not
    /// read is one whose flags we do not have either.
    pub fn unknown(name: impl Into<String>) -> Self {
        let name = name.into();
        let kind = kind_from_extension(&name);
        PluginEntry {
            name,
            enabled: false,
            kind,
            masters: Vec::new(),
            implicit: false,
        }
    }
}

/// The kind an extension implies, for a file whose header is unavailable.
pub fn kind_from_extension(name: &str) -> PluginKind {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".esm") {
        PluginKind::Master
    } else if lower.ends_with(".esl") {
        PluginKind::LightMaster
    } else {
        PluginKind::Normal
    }
}

/// Two names for the same plugin file.
///
/// The list is written by Windows tools onto a case-sensitive filesystem, so
/// `MyMod.esp` and `mymod.esp` are the same plugin to the game and different
/// strings to us. Every comparison here goes through this.
fn same_plugin(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// A plugin that loads before something it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterViolation {
    /// The plugin whose dependency is unsatisfied.
    pub plugin: String,
    /// The master it names.
    pub master: String,
    /// Why the order is wrong, in the terms the person reading it needs.
    pub reason: MasterProblem,
}

/// The two ways a master constraint fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterProblem {
    /// The master is in the list, but after the plugin that needs it.
    LoadsTooLate,
    /// The master is not in the list at all, or is present and disabled.
    Missing,
}

/// A game's plugin list, in the order the game will read it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOrder {
    entries: Vec<PluginEntry>,
}

impl PluginOrder {
    /// The list exactly as given, with no sorting applied.
    ///
    /// Used to adopt what a user already curated. Whatever they chose is the
    /// starting point, including an order this type would not have produced.
    pub fn adopt(entries: Vec<PluginEntry>) -> Self {
        PluginOrder { entries }
    }

    pub fn entries(&self) -> &[PluginEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Where a plugin sits, by the game's idea of the same name.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| same_plugin(&e.name, name))
    }

    pub fn get(&self, name: &str) -> Option<&PluginEntry> {
        self.position(name).map(|i| &self.entries[i])
    }

    /// Add plugins the list has never seen, leaving every existing entry where
    /// it is.
    ///
    /// This is the whole of what a deploy is allowed to do to somebody's order.
    /// A new plugin goes at the end of its own class — after the last master if
    /// it is one, at the very end if it is not — because that is the position
    /// that changes the fewest existing load decisions. A plugin already in the
    /// list is refreshed in place: its kind and masters come from the file that
    /// is now deployed, which may be a different version, but its position and
    /// its enabled state are the user's and are left alone.
    ///
    /// Returns the names actually added, so the interface can say what appeared
    /// rather than leaving someone to compare two lists.
    pub fn append_new(&mut self, incoming: impl IntoIterator<Item = PluginEntry>) -> Vec<String> {
        let mut added = Vec::new();
        for plugin in incoming {
            match self.position(&plugin.name) {
                Some(at) => {
                    // Known file, possibly a new version of it. Take what the
                    // file says about itself; keep what the user said about it.
                    let existing = &mut self.entries[at];
                    existing.kind = plugin.kind;
                    existing.masters = plugin.masters;
                    existing.implicit = plugin.implicit;
                }
                None => {
                    let at = self.insertion_point(plugin.kind);
                    added.push(plugin.name.clone());
                    self.entries.insert(at, plugin);
                }
            }
        }
        added
    }

    /// The end of the block this kind belongs to.
    ///
    /// Masters go after the last master, so a new master cannot land after the
    /// plugins that will name it. Everything else goes at the end.
    fn insertion_point(&self, kind: PluginKind) -> usize {
        if !kind.loads_as_master() {
            return self.entries.len();
        }
        self.entries
            .iter()
            .rposition(|e| e.kind.loads_as_master())
            .map(|last| last + 1)
            .unwrap_or(0)
    }

    /// Turn one plugin on or off, by the game's idea of the same name.
    ///
    /// Returns whether the list changed, so a caller can decline to write a file
    /// that would be identical.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        match self.position(name) {
            Some(at) if self.entries[at].enabled != enabled => {
                self.entries[at].enabled = enabled;
                true
            }
            _ => false,
        }
    }

    /// Move one plugin to a new index, shifting the rest.
    ///
    /// The user's own reordering, and the only thing that moves an entry that
    /// is already in the list. Out-of-range indices clamp rather than fail: the
    /// request came from a drag, and a drag past the end means the end.
    pub fn move_to(&mut self, name: &str, to: usize) -> bool {
        let Some(from) = self.position(name) else {
            return false;
        };
        let to = to.min(self.entries.len().saturating_sub(1));
        if from == to {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        true
    }

    /// Every master constraint this order breaks.
    ///
    /// Reported, never repaired. A plugin order is something a person curated in
    /// a tool that is not this one, and the mod manager that silently rewrites
    /// it is the one nobody trusts twice. An empty result is the only thing that
    /// makes the order safe to hand the game.
    ///
    /// A master that is present but disabled counts as missing, because that is
    /// what the game will see.
    pub fn violations(&self) -> Vec<MasterViolation> {
        let mut out = Vec::new();
        for (at, entry) in self.entries.iter().enumerate() {
            if !entry.enabled && !entry.implicit {
                // A plugin the game will not load cannot break anything by
                // sitting in the wrong place.
                continue;
            }
            for master in &entry.masters {
                let found = self
                    .entries
                    .iter()
                    .enumerate()
                    .find(|(_, e)| same_plugin(&e.name, master));
                let reason = match found {
                    Some((_, m)) if !m.enabled && !m.implicit => MasterProblem::Missing,
                    Some((i, _)) if i > at => MasterProblem::LoadsTooLate,
                    Some(_) => continue,
                    None => MasterProblem::Missing,
                };
                out.push(MasterViolation {
                    plugin: entry.name.clone(),
                    master: master.clone(),
                    reason,
                });
            }
        }
        out
    }

    /// The order the game will actually use, which is not always the one
    /// written down.
    ///
    /// Creation Engine loads the master block first regardless of the list, so a
    /// list interleaving the two describes something that cannot happen. This is
    /// the stable partition into master-block-then-the-rest, preserving relative
    /// order within each block, and it is what gets written and what the
    /// interface should show.
    pub fn as_the_game_reads_it(&self) -> Vec<&PluginEntry> {
        let (masters, rest): (Vec<_>, Vec<_>) =
            self.entries.iter().partition(|e| e.kind.loads_as_master());
        masters.into_iter().chain(rest).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(name: &str, kind: PluginKind, masters: &[&str]) -> PluginEntry {
        PluginEntry {
            name: name.to_string(),
            enabled: true,
            kind,
            masters: masters.iter().map(|m| m.to_string()).collect(),
            implicit: false,
        }
    }

    fn implicit(name: &str) -> PluginEntry {
        PluginEntry {
            implicit: true,
            ..plugin(name, PluginKind::Master, &[])
        }
    }

    #[test]
    fn a_new_plugin_goes_at_the_end_and_nothing_already_there_moves() {
        let mut order = PluginOrder::adopt(vec![
            implicit("Skyrim.esm"),
            plugin("Curated.esp", PluginKind::Normal, &[]),
            plugin("AlsoCurated.esp", PluginKind::Normal, &[]),
        ]);

        let added = order.append_new([plugin("New.esp", PluginKind::Normal, &[])]);

        assert_eq!(added, vec!["New.esp"]);
        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Skyrim.esm", "Curated.esp", "AlsoCurated.esp", "New.esp"],
            "the curated order is untouched and the new plugin is last"
        );
    }

    /// A new master cannot go at the very end: everything that names it is
    /// already above, so it would arrive breaking every one of them.
    #[test]
    fn a_new_master_lands_after_the_last_master_not_after_everything() {
        let mut order = PluginOrder::adopt(vec![
            implicit("Skyrim.esm"),
            plugin("Curated.esp", PluginKind::Normal, &[]),
        ]);

        order.append_new([plugin("New.esm", PluginKind::Master, &[])]);

        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Skyrim.esm", "New.esm", "Curated.esp"]);
    }

    #[test]
    fn a_plugin_already_in_the_list_keeps_its_place_and_its_switch() {
        let mut order = PluginOrder::adopt(vec![
            plugin("First.esp", PluginKind::Normal, &[]),
            PluginEntry {
                enabled: false,
                ..plugin("Known.esp", PluginKind::Normal, &[])
            },
        ]);

        // The same plugin, redeployed at a new version that gained a master.
        order.append_new([plugin("known.esp", PluginKind::Normal, &["Skyrim.esm"])]);

        assert_eq!(order.position("Known.esp"), Some(1), "it did not move");
        let entry = order.get("KNOWN.ESP").unwrap();
        assert!(!entry.enabled, "the user's switch is theirs");
        assert_eq!(
            entry.masters,
            vec!["Skyrim.esm"],
            "the file's word is taken"
        );
        assert_eq!(order.len(), 2, "no duplicate under a different casing");
    }

    #[test]
    fn a_master_loading_after_the_plugin_that_needs_it_is_reported() {
        let order = PluginOrder::adopt(vec![
            plugin("Needs.esp", PluginKind::Normal, &["Dependency.esp"]),
            plugin("Dependency.esp", PluginKind::Normal, &[]),
        ]);

        let violations = order.violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].plugin, "Needs.esp");
        assert_eq!(violations[0].master, "Dependency.esp");
        assert_eq!(violations[0].reason, MasterProblem::LoadsTooLate);
    }

    #[test]
    fn a_master_that_is_absent_or_switched_off_is_missing_either_way() {
        let order = PluginOrder::adopt(vec![
            PluginEntry {
                enabled: false,
                ..plugin("Off.esm", PluginKind::Master, &[])
            },
            plugin("NeedsOff.esp", PluginKind::Normal, &["Off.esm"]),
            plugin("NeedsAbsent.esp", PluginKind::Normal, &["NotHere.esm"]),
        ]);

        let violations = order.violations();
        assert_eq!(violations.len(), 2);
        assert!(violations
            .iter()
            .all(|v| v.reason == MasterProblem::Missing));
    }

    /// The game loads these whether the list mentions them or not, so a
    /// dependency on one is satisfied even with no `*` against it.
    #[test]
    fn an_implicit_master_satisfies_a_dependency_without_being_switched_on() {
        let order = PluginOrder::adopt(vec![
            PluginEntry {
                enabled: false,
                ..implicit("Skyrim.esm")
            },
            plugin("Mod.esp", PluginKind::Normal, &["Skyrim.esm"]),
        ]);

        assert!(order.violations().is_empty());
    }

    #[test]
    fn a_plugin_that_will_not_load_cannot_break_an_order() {
        let order = PluginOrder::adopt(vec![PluginEntry {
            enabled: false,
            ..plugin("Off.esp", PluginKind::Normal, &["NotHere.esm"])
        }]);

        assert!(order.violations().is_empty());
    }

    #[test]
    fn the_game_reads_every_master_first_whatever_the_list_says() {
        let order = PluginOrder::adopt(vec![
            plugin("A.esp", PluginKind::Normal, &[]),
            plugin("B.esm", PluginKind::Master, &[]),
            plugin("C.esl", PluginKind::LightMaster, &[]),
            plugin("D.esp", PluginKind::Normal, &[]),
        ]);

        let names: Vec<&str> = order
            .as_the_game_reads_it()
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["B.esm", "C.esl", "A.esp", "D.esp"],
            "master block first, relative order kept inside each block"
        );
    }

    #[test]
    fn moving_a_plugin_past_the_end_puts_it_at_the_end() {
        let mut order = PluginOrder::adopt(vec![
            plugin("A.esp", PluginKind::Normal, &[]),
            plugin("B.esp", PluginKind::Normal, &[]),
        ]);

        assert!(order.move_to("A.esp", 99));
        let names: Vec<&str> = order.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["B.esp", "A.esp"]);
        assert!(!order.move_to("Absent.esp", 0));
    }

    #[test]
    fn the_extension_is_the_fallback_when_a_header_could_not_be_read() {
        assert_eq!(PluginEntry::unknown("A.ESM").kind, PluginKind::Master);
        assert_eq!(PluginEntry::unknown("b.esl").kind, PluginKind::LightMaster);
        assert_eq!(PluginEntry::unknown("c.esp").kind, PluginKind::Normal);
        assert!(!PluginEntry::unknown("c.esp").enabled, "opt in, not out");
    }
}
