//! FOMOD: the conditional installer format used across the Bethesda modding
//! ecosystem and by Nexus tooling generally.
//!
//! Every other format this manager understands is a *shape* to be recognised —
//! a `natives/` directory, a set of `modinfo.ini` folders, a bare proxy DLL.
//! FOMOD is not a shape. It is a small program: the author declares steps,
//! groups and plugins, and declares conditions under which each of those
//! appears, is required, or is forbidden. Which files land is the result of
//! *running* that program against the user's answers.
//!
//! So these types describe a program, and they are deliberately faithful to
//! `ModuleConfig.xml` rather than to anything convenient. The translation into
//! the flat [`crate::modpack::ModBundle`] vocabulary happens in `apoc-modengine`,
//! where the archive is in hand; nothing here reads a file or knows one exists.
//!
//! Two properties are load-bearing and are worth stating where the types live:
//!
//! 1. **Which files an option installs is fixed; whether it applies is not.**
//!    A plugin's `<files>` are known the moment the document is parsed. Only its
//!    visibility and its type depend on other choices. That is what allows the
//!    payloads to be resolved once, at analyze time, and the conditions to be
//!    evaluated over ids alone.
//! 2. **A condition can be unknown.** Some dependencies ask about things this
//!    program cannot see — a game version it has no way to read, a file in a
//!    directory that has not been located. [`Truth`] has a third value for
//!    exactly that, and no evaluator is permitted to collapse it silently.

use serde::{Deserialize, Serialize};

/// A parsed `fomod/ModuleConfig.xml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FomodModule {
    /// `<moduleName>`.
    pub name: String,
    /// `<moduleImage path=>`, stored as the archive's own true-cased path.
    #[serde(default)]
    pub image: Option<String>,
    /// `<moduleDependencies>`: whether this mod is installable at all.
    #[serde(default)]
    pub module_dependencies: Option<CompositeDependency>,
    /// `<requiredInstallFiles>`: installed whatever the user chooses.
    #[serde(default)]
    pub required_install_files: Vec<FileSpec>,
    pub install_steps: Vec<InstallStep>,
    /// The order `<installSteps order=>` asks for.
    #[serde(default)]
    pub step_order: SortOrder,
    /// `<conditionalFileInstalls>`: extra files decided by the final flag state.
    #[serde(default)]
    pub conditional_installs: Vec<ConditionalPattern>,
    /// Constructs that parsed but could not be honoured exactly, in the author's
    /// own terms. These are shown to the user rather than swallowed: a silently
    /// degraded installer is indistinguishable from a correct one until the game
    /// fails to start.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// One page of the installer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStep {
    /// Stable id, `step-{n}` in document order.
    pub id: String,
    pub name: String,
    /// `<visible>`: when absent the step is always shown.
    #[serde(default)]
    pub visible: Option<CompositeDependency>,
    pub groups: Vec<PluginGroup>,
}

/// A set of plugins sharing a selection rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginGroup {
    /// Stable id, `step-{n}/group-{m}`.
    pub id: String,
    pub name: String,
    pub kind: GroupKind,
    #[serde(default)]
    pub order: SortOrder,
    pub plugins: Vec<Plugin>,
}

/// How many of a group's plugins may — or must — be chosen.
///
/// This is the distinction the older, inferred model could not draw. A Fluffy
/// radio set means "at most one of these"; FOMOD separates that from "exactly
/// one of these", and the difference decides whether the user may proceed
/// having chosen nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    /// Exactly one. The step is unanswered until one is chosen.
    SelectExactlyOne,
    /// At most one, possibly none.
    SelectAtMostOne,
    /// One or more. Unanswered while empty.
    SelectAtLeastOne,
    /// All of them, with no choice offered.
    SelectAll,
    /// Any number, including none.
    SelectAny,
}

impl GroupKind {
    /// True when the group must have an answer before the install may proceed.
    pub fn requires_answer(&self) -> bool {
        matches!(
            self,
            GroupKind::SelectExactlyOne | GroupKind::SelectAtLeastOne
        )
    }

    /// True when choosing one member must clear the others.
    pub fn is_one_of(&self) -> bool {
        matches!(
            self,
            GroupKind::SelectExactlyOne | GroupKind::SelectAtMostOne
        )
    }
}

/// One choice the user can make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plugin {
    /// Stable id derived from step, group and plugin *names* rather than
    /// positions, so inserting a step into a later release does not renumber
    /// every answer a user has already given. The same trade-off `carry.rs`
    /// documents for folder names applies: renaming a plugin breaks the match.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `<image path=>`, as the archive's own true-cased path.
    #[serde(default)]
    pub image: Option<String>,
    pub files: Vec<FileSpec>,
    /// `<conditionFlags>`: name/value pairs this plugin asserts while selected.
    #[serde(default)]
    pub condition_flags: Vec<(String, String)>,
    pub type_descriptor: TypeDescriptor,
}

/// What a plugin is to the user right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginTypeName {
    /// Always installed, not toggleable.
    Required,
    /// Freely chosen.
    Optional,
    /// Chosen by default, but the user may decline.
    Recommended,
    /// Must not be installed; shown with its reason rather than hidden, because
    /// a choice that silently vanishes reads as a bug.
    NotUsable,
    /// Not usable yet, but would be if other choices changed.
    CouldBeUsable,
}

/// How a plugin's type is decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeDescriptor {
    /// Used when no pattern matches.
    pub default: PluginTypeName,
    /// `<dependencyType>` patterns in **document order**. The first match wins,
    /// and that ordering is part of the format rather than an implementation
    /// detail — two patterns may both hold, and the author decided which one
    /// they meant by writing it first.
    #[serde(default)]
    pub patterns: Vec<(CompositeDependency, PluginTypeName)>,
}

impl TypeDescriptor {
    /// A descriptor with no conditions attached.
    pub fn plain(default: PluginTypeName) -> Self {
        TypeDescriptor {
            default,
            patterns: Vec::new(),
        }
    }
}

/// One `<file>` or `<folder>` an option installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSpec {
    /// Path inside the archive, separators normalised to `/`. Casing is the
    /// author's and is **not** to be trusted for lookups; resolution against the
    /// archive's real entries happens in `apoc-modengine`.
    pub source: String,
    /// Where it goes, game-relative. `None` means "mirror the source path".
    #[serde(default)]
    pub destination: Option<String>,
    /// Higher wins when two options write the same destination.
    #[serde(default)]
    pub priority: i32,
    /// True for `<folder>`, which installs every descendant of `source`.
    #[serde(default)]
    pub is_folder: bool,
    /// Installed even when its owning plugin is not selected.
    #[serde(default)]
    pub always_install: bool,
    /// Installed when its owning plugin is merely usable, not only when chosen.
    #[serde(default)]
    pub install_if_usable: bool,
}

/// A condition, as a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum CompositeDependency {
    And(Vec<CompositeDependency>),
    Or(Vec<CompositeDependency>),
    /// A flag set by some plugin holds this value.
    Flag {
        name: String,
        value: String,
    },
    /// A file in the game directory is in this state.
    File {
        path: String,
        state: FileState,
    },
    /// The game is at least this version.
    GameVersion(String),
    /// The mod manager is at least this version.
    ManagerVersion(String),
    /// Parsed, understood as a condition, but not answerable here — and
    /// carrying the author's own words so the user can be told what could not
    /// be checked. It is never quietly treated as true or as false.
    Undecidable(String),
}

/// What a `<fileDependency>` asserts about a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileState {
    Missing,
    Inactive,
    Active,
}

/// Three-valued truth.
///
/// A two-valued evaluator would have to decide what an unanswerable condition
/// means, and both answers are wrong: treating it as true installs files the
/// author gated, treating it as false withholds files the user is entitled to.
/// `Unknown` propagates instead, and each caller decides — usually by asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    pub fn is_true(self) -> bool {
        self == Truth::True
    }

    pub fn is_false(self) -> bool {
        self == Truth::False
    }

    /// `False` dominates, then `Unknown`. Empty is vacuously `True`.
    pub fn all(parts: impl IntoIterator<Item = Truth>) -> Truth {
        let mut seen_unknown = false;
        for p in parts {
            match p {
                Truth::False => return Truth::False,
                Truth::Unknown => seen_unknown = true,
                Truth::True => {}
            }
        }
        if seen_unknown {
            Truth::Unknown
        } else {
            Truth::True
        }
    }

    /// `True` dominates, then `Unknown`. Empty is vacuously `False`.
    pub fn any(parts: impl IntoIterator<Item = Truth>) -> Truth {
        let mut seen_unknown = false;
        for p in parts {
            match p {
                Truth::True => return Truth::True,
                Truth::Unknown => seen_unknown = true,
                Truth::False => {}
            }
        }
        if seen_unknown {
            Truth::Unknown
        } else {
            Truth::False
        }
    }
}

/// One `<pattern>` of `<conditionalFileInstalls>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionalPattern {
    /// Stable id, `@conditional-{n}`. The `@` prefix marks the synthetic
    /// options lowering creates, which cannot collide with a plugin's id.
    pub id: String,
    pub condition: CompositeDependency,
    pub files: Vec<FileSpec>,
}

/// How a list asks to be ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    /// Document order, exactly as written.
    #[default]
    Explicit,
    Ascending,
    Descending,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_is_false_when_any_part_is_false_even_beside_unknown() {
        assert_eq!(
            Truth::all([Truth::Unknown, Truth::False, Truth::True]),
            Truth::False
        );
    }

    #[test]
    fn all_is_unknown_when_nothing_is_false_but_something_is_unknown() {
        assert_eq!(Truth::all([Truth::True, Truth::Unknown]), Truth::Unknown);
    }

    #[test]
    fn any_is_true_when_any_part_is_true_even_beside_unknown() {
        assert_eq!(Truth::any([Truth::Unknown, Truth::True]), Truth::True);
    }

    #[test]
    fn any_is_unknown_when_nothing_is_true_but_something_is_unknown() {
        assert_eq!(Truth::any([Truth::False, Truth::Unknown]), Truth::Unknown);
    }

    #[test]
    fn empty_conjunction_holds_and_empty_disjunction_does_not() {
        assert_eq!(Truth::all([]), Truth::True);
        assert_eq!(Truth::any([]), Truth::False);
    }

    #[test]
    fn only_exactly_one_and_at_least_one_demand_an_answer() {
        assert!(GroupKind::SelectExactlyOne.requires_answer());
        assert!(GroupKind::SelectAtLeastOne.requires_answer());
        assert!(!GroupKind::SelectAtMostOne.requires_answer());
        assert!(!GroupKind::SelectAll.requires_answer());
        assert!(!GroupKind::SelectAny.requires_answer());
    }

    #[test]
    fn one_of_kinds_are_the_two_that_clear_their_siblings() {
        assert!(GroupKind::SelectExactlyOne.is_one_of());
        assert!(GroupKind::SelectAtMostOne.is_one_of());
        assert!(!GroupKind::SelectAtLeastOne.is_one_of());
        assert!(!GroupKind::SelectAny.is_one_of());
    }
}
