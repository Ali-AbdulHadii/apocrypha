//! Deciding which of an installer's options apply.
//!
//! A FOMOD's steps can depend on flags that its own plugins set, so answering
//! "which options are visible" needs the answer to "which options are selected",
//! which needs the visible ones. That circularity is resolved by iterating to a
//! fixed point rather than by evaluating once in document order.
//!
//! Three properties matter more here than anywhere else in the format, because
//! this is the code that decides what lands in someone's game directory.
//!
//! **It is pure.** Given the same module, the same probes and the same answers
//! it returns the same result, with no archive and no filesystem in reach. That
//! is what allows the wizard to re-evaluate on every click without a session
//! living anywhere, and what makes every case below testable by construction.
//!
//! **It is order-independent.** Plugin types are resolved against the flags as
//! they stood at the *start* of each pass, never against flags a sibling set
//! mid-pass. Without that, which of two plugins was written first would change
//! the outcome, and two runs over the same archive could disagree.
//!
//! **It terminates, or it refuses.** An installer can contradict itself: "A is
//! required unless flag F is set" on a plugin that sets F. There is no answer to
//! that, and the honest response is to say so rather than to iterate until
//! something gives up. Both non-convergence and a two-state oscillation are
//! detected and reported as an installer that does not settle.

use crate::error::{ModEngineError, Result};
use apoc_domain::fomod::{
    CompositeDependency, FileState, FomodModule, GroupKind, PluginTypeName, SortOrder, Truth,
};
use apoc_domain::Selection;
use std::collections::{BTreeMap, BTreeSet};

/// Passes before an installer is declared unstable. Real ones settle in two;
/// this is a bound on cost, not a target.
const MAX_PASSES: usize = 16;

/// Facts about the world a condition can ask about, gathered before evaluation
/// so that evaluation itself stays pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probes {
    /// Game-relative path, lowercased, to what was found there.
    pub file_states: BTreeMap<String, FileState>,
    /// The installed game's version, when anything knows it. Nothing does yet,
    /// which is why version conditions are answered `Unknown`.
    pub game_version: Option<String>,
}

/// What the installer looks like given a set of answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalState {
    /// Steps to show, in the order they should appear.
    pub steps: Vec<VisibleStep>,
    /// Flags asserted by everything currently applying.
    pub flags: BTreeMap<String, String>,
    /// Option ids that would be installed if this were confirmed now.
    pub resolved: Selection,
    /// Groups that must be answered and are not, by label.
    pub unanswered: Vec<String>,
    /// Why the mod cannot be installed at all, when that is the case.
    pub blocked: Option<String>,
    /// Conditions that could not be checked, in terms a user can read.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleStep {
    pub id: String,
    pub name: String,
    pub groups: Vec<VisibleGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleGroup {
    pub id: String,
    pub name: String,
    pub kind: GroupKind,
    pub plugins: Vec<VisiblePlugin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisiblePlugin {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// The type in force right now, which a condition may have changed.
    pub effective_type: PluginTypeName,
    pub selected: bool,
    /// True when the user cannot change this one: required, or forbidden.
    pub locked: bool,
}

/// Look up every file an installer asks about, once, before evaluating.
///
/// The only function in this module that touches a filesystem, and it is
/// deliberately separate: everything else stays a pure function of what this
/// found, so the whole conditional system is testable without a game installed.
///
/// A path that cannot be checked is left out rather than recorded as missing.
/// Recording it as missing would silently satisfy every "must not be present"
/// condition in every installer, which is exactly the kind of wrong answer that
/// installs files nobody chose.
pub fn probe(module: &FomodModule, game_dir: Option<&std::path::Path>) -> Probes {
    let mut probes = Probes::default();
    let Some(dir) = game_dir else {
        return probes;
    };

    let mut wanted: BTreeSet<String> = BTreeSet::new();
    collect_files(module.module_dependencies.as_ref(), &mut wanted);
    for step in &module.install_steps {
        collect_files(step.visible.as_ref(), &mut wanted);
        for group in &step.groups {
            for plugin in &group.plugins {
                for (condition, _) in &plugin.type_descriptor.patterns {
                    collect_files(Some(condition), &mut wanted);
                }
            }
        }
    }
    for pattern in &module.conditional_installs {
        collect_files(Some(&pattern.condition), &mut wanted);
    }

    for path in wanted {
        // A game directory is case-insensitive on Windows and not on Linux, and
        // the manifest's casing is the author's. Only an exact hit is recorded;
        // anything else stays unknown rather than guessed.
        let candidate = dir.join(&path);
        if candidate.is_file() {
            probes.file_states.insert(path, FileState::Active);
        }
    }
    probes
}

fn collect_files(condition: Option<&CompositeDependency>, out: &mut BTreeSet<String>) {
    let Some(condition) = condition else { return };
    match condition {
        CompositeDependency::And(parts) | CompositeDependency::Or(parts) => {
            for p in parts {
                collect_files(Some(p), out);
            }
        }
        CompositeDependency::File { path, .. } => {
            out.insert(path.to_ascii_lowercase());
        }
        _ => {}
    }
}

/// Evaluate an installer against a set of chosen plugin ids.
pub fn evaluate(
    module: &FomodModule,
    probes: &Probes,
    chosen: &BTreeSet<String>,
) -> Result<EvalState> {
    let mut warnings = Vec::new();

    // Whether the mod is installable at all comes first: if it is not, the
    // steps below it are moot.
    let blocked = match &module.module_dependencies {
        Some(condition) => match truth(condition, &BTreeMap::new(), probes) {
            Truth::False => Some(describe(condition)),
            Truth::Unknown => {
                // Refusing on a condition nobody can check would block a mod
                // that may well be fine. Saying so and continuing leaves the
                // decision with the person who can actually look.
                warnings.push(format!(
                    "This mod says it needs {}. Apocrypha cannot check that.",
                    describe(condition)
                ));
                None
            }
            Truth::True => None,
        },
        None => None,
    };

    let mut flags: BTreeMap<String, String> = BTreeMap::new();
    let mut previous: Option<(Vec<String>, BTreeSet<String>)> = None;
    let mut before_last: Option<(Vec<String>, BTreeSet<String>)> = None;
    let mut settled: Option<(Vec<String>, BTreeSet<String>)> = None;

    for _ in 0..MAX_PASSES {
        // Which steps are on screen, given the flags as they stand.
        let visible: Vec<String> = ordered_steps(module)
            .iter()
            .filter(|step| match &step.visible {
                // A step whose condition cannot be checked is shown. Hiding it
                // would silently withhold a choice the author meant to offer.
                Some(c) => !truth(c, &flags, probes).is_false(),
                None => true,
            })
            .map(|s| s.id.clone())
            .collect();

        // What applies, given those steps and the answers within them.
        let mut applied: BTreeSet<String> = BTreeSet::new();
        for step in ordered_steps(module) {
            if !visible.contains(&step.id) {
                continue;
            }
            for group in &step.groups {
                for plugin in &group.plugins {
                    // Types resolve against the flags from the start of this
                    // pass, never against one a sibling set moments ago. This
                    // is what makes the result independent of document order.
                    let kind = effective_type(plugin, &flags, probes);
                    let forced =
                        kind == PluginTypeName::Required || group.kind == GroupKind::SelectAll;
                    let forbidden = kind == PluginTypeName::NotUsable;
                    if forbidden {
                        continue;
                    }
                    if forced || chosen.contains(&plugin.id) {
                        applied.insert(plugin.id.clone());
                    }
                }
            }
        }

        let state = (visible.clone(), applied.clone());
        if previous.as_ref() == Some(&state) {
            settled = Some(state);
            break;
        }
        // A two-state cycle: "A is required while F is unset" on a plugin that
        // sets F. Iterating further only alternates, so it is reported as the
        // contradiction it is rather than as a timeout.
        if before_last.as_ref() == Some(&state) {
            return Err(ModEngineError::FomodUnsupported {
                feature: "conditions that contradict themselves".to_string(),
                detail: "the installer's options keep changing each other, so there is no \
                         combination that satisfies all of them"
                    .to_string(),
            });
        }

        let mut next_flags = BTreeMap::new();
        for step in ordered_steps(module) {
            for group in &step.groups {
                for plugin in &group.plugins {
                    if applied.contains(&plugin.id) {
                        for (name, value) in &plugin.condition_flags {
                            next_flags.insert(name.clone(), value.clone());
                        }
                    }
                }
            }
        }

        before_last = previous.take();
        previous = Some(state);
        flags = next_flags;
    }

    let Some((visible, applied)) = settled else {
        return Err(ModEngineError::FomodUnsupported {
            feature: "conditions that do not settle".to_string(),
            detail: format!(
                "the installer was re-evaluated {MAX_PASSES} times without reaching a stable set \
                 of options"
            ),
        });
    };

    // Build what the wizard shows, now that the flags are known to be stable.
    let mut steps = Vec::new();
    let mut unanswered = Vec::new();
    for step in ordered_steps(module) {
        if !visible.contains(&step.id) {
            continue;
        }
        let mut groups = Vec::new();
        for group in &step.groups {
            let mut plugins = Vec::new();
            let mut answered = false;
            for plugin in sorted_plugins(group) {
                let kind = effective_type(plugin, &flags, probes);
                let selected = applied.contains(&plugin.id);
                answered |= selected;
                plugins.push(VisiblePlugin {
                    id: plugin.id.clone(),
                    name: plugin.name.clone(),
                    description: plugin.description.clone(),
                    effective_type: kind,
                    selected,
                    locked: matches!(kind, PluginTypeName::Required | PluginTypeName::NotUsable)
                        || group.kind == GroupKind::SelectAll,
                });
            }
            if group.kind.requires_answer() && !answered {
                unanswered.push(format!("{} · {}", step.name, group.name));
            }
            groups.push(VisibleGroup {
                id: group.id.clone(),
                name: group.name.clone(),
                kind: group.kind,
                plugins,
            });
        }
        steps.push(VisibleStep {
            id: step.id.clone(),
            name: step.name.clone(),
            groups,
        });
    }

    // Patterns that install files with no choice attached, decided once against
    // the settled flags. They set no flags themselves, so they cannot feed back
    // into the loop above and are safe to resolve last.
    let mut resolved = Selection {
        chosen: applied.clone(),
    };
    // Files the installer gives everybody, which lowering carries as a single
    // forced option rather than as a choice.
    resolved.insert(super::lower::REQUIRED_ID.to_string());
    for pattern in &module.conditional_installs {
        match truth(&pattern.condition, &flags, probes) {
            Truth::True => {
                resolved.insert(pattern.id.clone());
            }
            Truth::False => {}
            Truth::Unknown => {
                // The one place a guess would change which files land with no
                // choice on screen. It stays out, and is named so the user can
                // decide for themselves.
                warnings.push(format!(
                    "The installer adds extra files when {}, which Apocrypha cannot check. \
                     They are left out.",
                    describe(&pattern.condition)
                ));
            }
        }
    }

    Ok(EvalState {
        steps,
        flags,
        resolved,
        unanswered,
        blocked,
        warnings,
    })
}

/// The installer's steps in the order it asked for, stably.
fn ordered_steps(module: &FomodModule) -> Vec<&apoc_domain::fomod::InstallStep> {
    let mut steps: Vec<_> = module.install_steps.iter().collect();
    match module.step_order {
        SortOrder::Explicit => {}
        SortOrder::Ascending => steps.sort_by(|a, b| a.name.cmp(&b.name)),
        SortOrder::Descending => steps.sort_by(|a, b| b.name.cmp(&a.name)),
    }
    steps
}

fn sorted_plugins(group: &apoc_domain::fomod::PluginGroup) -> Vec<&apoc_domain::fomod::Plugin> {
    let mut plugins: Vec<_> = group.plugins.iter().collect();
    match group.order {
        SortOrder::Explicit => {}
        SortOrder::Ascending => plugins.sort_by(|a, b| a.name.cmp(&b.name)),
        SortOrder::Descending => plugins.sort_by(|a, b| b.name.cmp(&a.name)),
    }
    plugins
}

/// A plugin's type under the current flags: the first matching pattern in
/// document order, or its default.
fn effective_type(
    plugin: &apoc_domain::fomod::Plugin,
    flags: &BTreeMap<String, String>,
    probes: &Probes,
) -> PluginTypeName {
    for (condition, kind) in &plugin.type_descriptor.patterns {
        // An unknown condition is not a match, so the default stands. It is not
        // treated as a failure either: the author wrote a default for exactly
        // the case where their condition does not hold.
        if truth(condition, flags, probes).is_true() {
            return *kind;
        }
    }
    plugin.type_descriptor.default
}

/// Evaluate one condition.
fn truth(
    condition: &CompositeDependency,
    flags: &BTreeMap<String, String>,
    probes: &Probes,
) -> Truth {
    match condition {
        CompositeDependency::And(parts) => {
            Truth::all(parts.iter().map(|p| truth(p, flags, probes)))
        }
        CompositeDependency::Or(parts) => Truth::any(parts.iter().map(|p| truth(p, flags, probes))),
        CompositeDependency::Flag { name, value } => {
            // An absent flag is an unset flag, which the format spells as the
            // empty string. This is knowable, so it is never Unknown.
            let current = flags.get(name).map(String::as_str).unwrap_or("");
            if current.eq_ignore_ascii_case(value) {
                Truth::True
            } else {
                Truth::False
            }
        }
        CompositeDependency::File { path, state } => {
            match probes.file_states.get(&path.to_ascii_lowercase()) {
                Some(found) => {
                    if found == state {
                        Truth::True
                    } else {
                        Truth::False
                    }
                }
                // Nothing looked, so nothing is known. Not "the file is
                // missing", which would silently satisfy every Missing check.
                None => Truth::Unknown,
            }
        }
        CompositeDependency::GameVersion(_) | CompositeDependency::ManagerVersion(_) => {
            Truth::Unknown
        }
        CompositeDependency::Undecidable(_) => Truth::Unknown,
    }
}

/// A condition in words, for a message a user will read.
fn describe(condition: &CompositeDependency) -> String {
    match condition {
        CompositeDependency::And(parts) => join(parts, " and "),
        CompositeDependency::Or(parts) => join(parts, " or "),
        CompositeDependency::Flag { name, value } if value.is_empty() => {
            format!("\"{name}\" is not set")
        }
        CompositeDependency::Flag { name, value } => format!("\"{name}\" is \"{value}\""),
        CompositeDependency::File { path, state } => match state {
            FileState::Missing => format!("{path} is not installed"),
            FileState::Inactive => format!("{path} is installed but inactive"),
            FileState::Active => format!("{path} is installed"),
        },
        CompositeDependency::GameVersion(v) => format!("the game is version {v} or newer"),
        CompositeDependency::ManagerVersion(v) => {
            format!("the mod manager is version {v} or newer")
        }
        CompositeDependency::Undecidable(what) => format!("a condition of type \"{what}\""),
    }
}

fn join(parts: &[CompositeDependency], sep: &str) -> String {
    let described: Vec<String> = parts.iter().map(describe).collect();
    match described.len() {
        0 => "nothing in particular".to_string(),
        1 => described.into_iter().next().unwrap_or_default(),
        _ => described.join(sep),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::fomod::{FileSpec, InstallStep, Plugin, PluginGroup, TypeDescriptor};

    fn plugin(name: &str, kind: PluginTypeName) -> Plugin {
        Plugin {
            id: name.to_ascii_lowercase(),
            name: name.to_string(),
            description: None,
            image: None,
            files: vec![FileSpec {
                source: format!("{name}.esp"),
                destination: None,
                priority: 0,
                is_folder: false,
                always_install: false,
                install_if_usable: false,
            }],
            condition_flags: Vec::new(),
            type_descriptor: TypeDescriptor::plain(kind),
        }
    }

    fn group(name: &str, kind: GroupKind, plugins: Vec<Plugin>) -> PluginGroup {
        PluginGroup {
            id: format!("group-{name}"),
            name: name.to_string(),
            kind,
            order: SortOrder::Explicit,
            plugins,
        }
    }

    fn step(
        name: &str,
        visible: Option<CompositeDependency>,
        groups: Vec<PluginGroup>,
    ) -> InstallStep {
        InstallStep {
            id: format!("step-{name}"),
            name: name.to_string(),
            visible,
            groups,
        }
    }

    fn module(steps: Vec<InstallStep>) -> FomodModule {
        FomodModule {
            name: "M".into(),
            image: None,
            module_dependencies: None,
            required_install_files: Vec::new(),
            install_steps: steps,
            step_order: SortOrder::Explicit,
            conditional_installs: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn chosen(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn flag(name: &str, value: &str) -> CompositeDependency {
        CompositeDependency::Flag {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn a_flag_set_in_one_step_reveals_the_next() {
        let mut setter = plugin("Slim", PluginTypeName::Optional);
        setter.condition_flags = vec![("body".into(), "slim".into())];

        let m = module(vec![
            step(
                "Body",
                None,
                vec![group("Shape", GroupKind::SelectAtMostOne, vec![setter])],
            ),
            step(
                "Slim extras",
                Some(flag("body", "slim")),
                vec![group(
                    "Extras",
                    GroupKind::SelectAny,
                    vec![plugin("Belt", PluginTypeName::Optional)],
                )],
            ),
        ]);

        let hidden = evaluate(&m, &Probes::default(), &chosen(&[])).unwrap();
        assert_eq!(hidden.steps.len(), 1, "the second step is gated");

        let shown = evaluate(&m, &Probes::default(), &chosen(&["slim"])).unwrap();
        assert_eq!(shown.steps.len(), 2);
        assert_eq!(shown.flags.get("body").map(String::as_str), Some("slim"));
    }

    #[test]
    fn unchoosing_the_flag_prunes_the_answers_that_depended_on_it() {
        let mut setter = plugin("Slim", PluginTypeName::Optional);
        setter.condition_flags = vec![("body".into(), "slim".into())];
        let m = module(vec![
            step(
                "Body",
                None,
                vec![group("Shape", GroupKind::SelectAtMostOne, vec![setter])],
            ),
            step(
                "Slim extras",
                Some(flag("body", "slim")),
                vec![group(
                    "Extras",
                    GroupKind::SelectAny,
                    vec![plugin("Belt", PluginTypeName::Optional)],
                )],
            ),
        ]);

        // The belt was chosen while its step was visible, and the body choice
        // is then withdrawn. The belt must not still install.
        let state = evaluate(&m, &Probes::default(), &chosen(&["belt"])).unwrap();
        assert!(!state.resolved.contains("belt"), "{:?}", state.resolved);
    }

    #[test]
    fn a_condition_can_promote_a_plugin_to_required_and_lock_it() {
        let mut setter = plugin("Core", PluginTypeName::Optional);
        setter.condition_flags = vec![("core".into(), "yes".into())];

        let mut dependent = plugin("Patch", PluginTypeName::Optional);
        dependent.type_descriptor = TypeDescriptor {
            default: PluginTypeName::Optional,
            patterns: vec![(flag("core", "yes"), PluginTypeName::Required)],
        };

        let m = module(vec![step(
            "S",
            None,
            vec![group("G", GroupKind::SelectAny, vec![setter, dependent])],
        )]);

        let state = evaluate(&m, &Probes::default(), &chosen(&["core"])).unwrap();
        let patch = &state.steps[0].groups[0].plugins[1];
        assert_eq!(patch.effective_type, PluginTypeName::Required);
        assert!(patch.locked);
        assert!(patch.selected, "a required option installs without a click");
    }

    #[test]
    fn a_forbidden_plugin_is_never_installed_even_if_it_was_chosen() {
        let m = module(vec![step(
            "S",
            None,
            vec![group(
                "G",
                GroupKind::SelectAny,
                vec![plugin("Legacy", PluginTypeName::NotUsable)],
            )],
        )]);

        let state = evaluate(&m, &Probes::default(), &chosen(&["legacy"])).unwrap();
        assert!(!state.resolved.contains("legacy"));
        assert!(state.steps[0].groups[0].plugins[0].locked);
    }

    #[test]
    fn the_first_matching_pattern_wins_as_written() {
        let mut setter = plugin("Core", PluginTypeName::Optional);
        setter.condition_flags = vec![("core".into(), "yes".into())];

        let mut dependent = plugin("Patch", PluginTypeName::Optional);
        dependent.type_descriptor = TypeDescriptor {
            default: PluginTypeName::Optional,
            // Both hold once `core` is set. The author decided which they meant
            // by writing it first.
            patterns: vec![
                (flag("core", "yes"), PluginTypeName::Recommended),
                (flag("core", "yes"), PluginTypeName::Required),
            ],
        };

        let m = module(vec![step(
            "S",
            None,
            vec![group("G", GroupKind::SelectAny, vec![setter, dependent])],
        )]);

        let state = evaluate(&m, &Probes::default(), &chosen(&["core"])).unwrap();
        assert_eq!(
            state.steps[0].groups[0].plugins[1].effective_type,
            PluginTypeName::Recommended
        );
    }

    #[test]
    fn document_order_does_not_change_the_outcome() {
        // Two plugins whose flags refer to each other's conditions. Reversing
        // the order they were written in must produce the same answer, which is
        // what reading flags from the start of a pass buys.
        let build = |reversed: bool| {
            let mut a = plugin("A", PluginTypeName::Optional);
            a.condition_flags = vec![("a".into(), "on".into())];
            let mut b = plugin("B", PluginTypeName::Optional);
            b.condition_flags = vec![("b".into(), "on".into())];
            b.type_descriptor = TypeDescriptor {
                default: PluginTypeName::Optional,
                patterns: vec![(flag("a", "on"), PluginTypeName::Required)],
            };
            let plugins = if reversed { vec![b, a] } else { vec![a, b] };
            module(vec![step(
                "S",
                None,
                vec![group("G", GroupKind::SelectAny, plugins)],
            )])
        };

        let forwards = evaluate(&build(false), &Probes::default(), &chosen(&["a"])).unwrap();
        let backwards = evaluate(&build(true), &Probes::default(), &chosen(&["a"])).unwrap();
        assert_eq!(forwards.resolved, backwards.resolved);
        assert_eq!(forwards.flags, backwards.flags);
    }

    #[test]
    fn an_installer_that_contradicts_itself_is_refused_rather_than_looped_over() {
        // "Required while F is unset", on the plugin that sets F. Selecting it
        // unsets the condition that selected it, forever.
        let mut contradictory = plugin("Paradox", PluginTypeName::Optional);
        contradictory.condition_flags = vec![("f".into(), "on".into())];
        contradictory.type_descriptor = TypeDescriptor {
            default: PluginTypeName::Optional,
            patterns: vec![(flag("f", ""), PluginTypeName::Required)],
        };

        let m = module(vec![step(
            "S",
            None,
            vec![group("G", GroupKind::SelectAny, vec![contradictory])],
        )]);

        // The assertion that matters is that this returns at all.
        match evaluate(&m, &Probes::default(), &chosen(&[])) {
            Err(ModEngineError::FomodUnsupported { feature, .. }) => {
                assert!(feature.contains("contradict"), "{feature}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_group_that_must_be_answered_is_reported_until_it_is() {
        let m = module(vec![step(
            "Body",
            None,
            vec![group(
                "Shape",
                GroupKind::SelectExactlyOne,
                vec![plugin("Slim", PluginTypeName::Optional)],
            )],
        )]);

        let empty = evaluate(&m, &Probes::default(), &chosen(&[])).unwrap();
        assert_eq!(empty.unanswered, vec!["Body · Shape"]);

        let answered = evaluate(&m, &Probes::default(), &chosen(&["slim"])).unwrap();
        assert!(answered.unanswered.is_empty());
    }

    #[test]
    fn a_group_that_need_not_be_answered_is_never_reported() {
        let m = module(vec![step(
            "S",
            None,
            vec![group(
                "Extras",
                GroupKind::SelectAtMostOne,
                vec![plugin("Belt", PluginTypeName::Optional)],
            )],
        )]);
        assert!(evaluate(&m, &Probes::default(), &chosen(&[]))
            .unwrap()
            .unanswered
            .is_empty());
    }

    #[test]
    fn conditional_files_are_decided_against_the_settled_flags() {
        use apoc_domain::fomod::ConditionalPattern;

        let mut setter = plugin("Slim", PluginTypeName::Optional);
        setter.condition_flags = vec![("body".into(), "slim".into())];

        let mut m = module(vec![step(
            "S",
            None,
            vec![group("G", GroupKind::SelectAtMostOne, vec![setter])],
        )]);
        m.conditional_installs = vec![ConditionalPattern {
            id: "@conditional-0".into(),
            condition: flag("body", "slim"),
            files: Vec::new(),
        }];

        let without = evaluate(&m, &Probes::default(), &chosen(&[])).unwrap();
        assert!(!without.resolved.contains("@conditional-0"));

        let with = evaluate(&m, &Probes::default(), &chosen(&["slim"])).unwrap();
        assert!(with.resolved.contains("@conditional-0"));
    }

    #[test]
    fn conditional_files_behind_an_uncheckable_condition_are_left_out_and_named() {
        use apoc_domain::fomod::ConditionalPattern;

        let mut m = module(vec![]);
        m.conditional_installs = vec![ConditionalPattern {
            id: "@conditional-0".into(),
            condition: CompositeDependency::GameVersion("1.6.640".into()),
            files: Vec::new(),
        }];

        let state = evaluate(&m, &Probes::default(), &chosen(&[])).unwrap();
        assert!(!state.resolved.contains("@conditional-0"));
        assert!(
            state.warnings.iter().any(|w| w.contains("1.6.640")),
            "{:?}",
            state.warnings
        );
    }

    #[test]
    fn a_mod_whose_requirements_are_unmet_says_which_ones() {
        let mut m = module(vec![]);
        m.module_dependencies = Some(CompositeDependency::File {
            path: "skse64_loader.exe".into(),
            state: FileState::Active,
        });

        let mut probes = Probes::default();
        probes
            .file_states
            .insert("skse64_loader.exe".into(), FileState::Missing);

        let state = evaluate(&m, &probes, &chosen(&[])).unwrap();
        let blocked = state.blocked.expect("blocked");
        assert!(blocked.contains("skse64_loader.exe"), "{blocked}");
    }

    #[test]
    fn a_requirement_nobody_can_check_warns_instead_of_blocking() {
        let mut m = module(vec![]);
        m.module_dependencies = Some(CompositeDependency::GameVersion("1.6.640".into()));

        let state = evaluate(&m, &Probes::default(), &chosen(&[])).unwrap();
        assert!(
            state.blocked.is_none(),
            "a mod that may be fine is not blocked"
        );
        assert!(state.warnings.iter().any(|w| w.contains("1.6.640")));
    }

    #[test]
    fn an_unset_flag_is_the_empty_string_rather_than_unknown() {
        // The format spells "unset" as the empty value, and that is knowable.
        assert_eq!(
            truth(&flag("never-set", ""), &BTreeMap::new(), &Probes::default()),
            Truth::True
        );
        assert_eq!(
            truth(
                &flag("never-set", "on"),
                &BTreeMap::new(),
                &Probes::default()
            ),
            Truth::False
        );
    }

    #[test]
    fn probing_finds_a_file_that_is_there_and_says_nothing_about_one_that_is_not() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("skse64_loader.exe"), b"x").unwrap();

        let mut m = module(vec![]);
        m.module_dependencies = Some(CompositeDependency::And(vec![
            CompositeDependency::File {
                path: "skse64_loader.exe".into(),
                state: FileState::Active,
            },
            CompositeDependency::File {
                path: "never_installed.dll".into(),
                state: FileState::Missing,
            },
        ]));

        let probes = probe(&m, Some(dir.path()));
        assert_eq!(
            probes.file_states.get("skse64_loader.exe"),
            Some(&FileState::Active)
        );
        assert!(
            !probes.file_states.contains_key("never_installed.dll"),
            "an absent file stays unknown: calling it missing would satisfy \
             every must-not-be-present check ever written"
        );
    }

    #[test]
    fn probing_without_a_game_directory_knows_nothing_rather_than_guessing() {
        let mut m = module(vec![]);
        m.module_dependencies = Some(CompositeDependency::File {
            path: "x.dll".into(),
            state: FileState::Missing,
        });
        assert!(probe(&m, None).file_states.is_empty());
    }

    #[test]
    fn a_file_nobody_looked_for_is_unknown_rather_than_missing() {
        // Answering Missing would silently satisfy every "must not be present"
        // check in every installer.
        let condition = CompositeDependency::File {
            path: "some.dll".into(),
            state: FileState::Missing,
        };
        assert_eq!(
            truth(&condition, &BTreeMap::new(), &Probes::default()),
            Truth::Unknown
        );
    }
}
