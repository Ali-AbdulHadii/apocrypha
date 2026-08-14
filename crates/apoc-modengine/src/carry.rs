//! Carrying an install selection across an update.
//!
//! When a mod is updated, the archive is new but the choices someone made in
//! the wizard usually still apply: they picked a body variant and two addons,
//! and they want the same body variant and the same two addons. Making them
//! rebuild that from memory on every release is the difference between an
//! update being a click and being a chore.
//!
//! Options are matched by [`ModOption::id`], which is derived from the folder
//! name inside the archive. That is the strongest identity available — nothing
//! in these archives carries a stable machine id — and it has a consequence
//! worth stating: **an author who renames a folder breaks the match**, and the
//! option reads as removed rather than as renamed. Guessing at renames by
//! position or by fuzzy name would silently install something the user did not
//! choose, which is worse than asking.
//!
//! Nothing here decides whether to show the wizard; it reports what carried,
//! what did not, and what is now undecided, and the caller chooses.

use apoc_domain::{ModBundle, ModOption, SelectMode, Selection};

use crate::plan::default_selection;

/// The outcome of moving a selection onto a new version of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarriedSelection {
    /// The selection to apply, already including the new bundle's forced
    /// options.
    pub selection: Selection,
    /// Ids that were chosen before and do not exist in the new bundle, sorted.
    /// Usually an option the author removed or renamed.
    pub dropped: Vec<String>,
    /// Radio sets in the new bundle with nothing chosen, sorted. Either the
    /// previously chosen variant is gone, or the set is new in this version.
    /// Each one is a question only a person can answer.
    pub undecided: Vec<String>,
}

impl CarriedSelection {
    /// True when the update can be applied without asking anything.
    ///
    /// Deliberately strict: a dropped option does not technically block an
    /// install, but it means the result differs from what the person chose last
    /// time, and applying that silently is how someone ends up with a mod that
    /// quietly stopped including the part they wanted.
    pub fn is_complete(&self) -> bool {
        self.dropped.is_empty() && self.undecided.is_empty()
    }
}

/// True for options a person can actually choose.
///
/// `Info` entries are covers and warnings that are never deployed, and `Forced`
/// entries come back from [`default_selection`] regardless, so neither is worth
/// carrying explicitly.
fn is_choosable(o: &ModOption) -> bool {
    o.deployable && !matches!(o.select_mode, SelectMode::Info)
}

/// Move `previous` onto `new_bundle`, keeping every choice that still exists.
pub fn carry_selection(new_bundle: &ModBundle, previous: &Selection) -> CarriedSelection {
    let mut selection = default_selection(new_bundle);
    let mut dropped = Vec::new();

    for id in &previous.chosen {
        match new_bundle.options().find(|o| &o.id == id) {
            Some(o) if is_choosable(o) => {
                selection.insert(o.id.clone());
            }
            // Present but no longer selectable — an option demoted to a cover or
            // a header. It cannot be carried and the person should know.
            Some(_) => dropped.push(id.clone()),
            None => dropped.push(id.clone()),
        }
    }

    // Every radio set the new bundle offers must end up with exactly one choice.
    // A set with none is an unanswered question; the wizard has to open.
    let mut undecided = Vec::new();
    for o in new_bundle.options() {
        let Some(set) = &o.radio_set else { continue };
        if undecided.contains(set) {
            continue;
        }
        let decided = new_bundle
            .options()
            .filter(|c| c.radio_set.as_ref() == Some(set))
            .any(|c| selection.contains(&c.id));
        if !decided {
            undecided.push(set.clone());
        }
    }

    // A conditional installer can invalidate an answer without removing the
    // option: the step it lives in may no longer be reachable under the new
    // version's conditions. Nothing above can see that, because it is a
    // question about the whole graph rather than about one option.
    if let Some(module) = &new_bundle.fomod {
        reconcile_with_conditions(module, &mut selection, &mut dropped, &mut undecided);
    }

    dropped.sort();
    dropped.dedup();
    undecided.sort();
    undecided.dedup();

    CarriedSelection {
        selection,
        dropped,
        undecided,
    }
}

/// Re-run a carried selection through the new version's conditions.
///
/// Answers that are no longer reachable are dropped and reported rather than
/// honoured, and groups the new version insists on are reported as unanswered.
/// Both mean the wizard opens, which is the point: a conditional installer can
/// change what a previous answer means without changing its name.
fn reconcile_with_conditions(
    module: &apoc_domain::fomod::FomodModule,
    selection: &mut Selection,
    dropped: &mut Vec<String>,
    undecided: &mut Vec<String>,
) {
    // No game directory here, so file conditions stay unknown and any step
    // gated on one is shown. Carrying is a question about the archive, and
    // showing a step the user may not need is recoverable; hiding one they do
    // is not.
    let probes = crate::fomod::eval::Probes::default();
    let state = match crate::fomod::eval::evaluate(module, &probes, &selection.chosen) {
        Ok(state) => state,
        // An installer whose conditions do not settle cannot be carried onto at
        // all. Reported as one unanswered question so the wizard opens and the
        // refusal is shown there rather than swallowed here.
        Err(_) => {
            undecided.push("this installer's conditions".to_string());
            return;
        }
    };

    for id in &selection.chosen {
        let reachable = state.steps.iter().any(|s| {
            s.groups
                .iter()
                .any(|g| g.plugins.iter().any(|p| &p.id == id))
        });
        // Synthetic ids (required files, conditional patterns) belong to no
        // step and are decided by the evaluator, not by a person.
        if !reachable && !id.starts_with('@') && !state.resolved.contains(id) {
            dropped.push(id.clone());
        }
    }

    undecided.extend(state.unanswered.iter().cloned());
    // What the evaluator resolved is what would install, including options the
    // conditions force and excluding ones they forbid.
    *selection = state.resolved;
}

#[cfg(test)]
mod fomod_tests {
    use super::*;
    use apoc_domain::fomod::{
        FomodModule, GroupKind, InstallStep, Plugin, PluginGroup, PluginTypeName, SortOrder,
        TypeDescriptor,
    };
    use apoc_domain::{InstallerModel, OptionGroup};

    fn plugin(name: &str, flags: &[(&str, &str)]) -> Plugin {
        Plugin {
            id: name.to_ascii_lowercase(),
            name: name.to_string(),
            description: None,
            image: None,
            files: Vec::new(),
            condition_flags: flags
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            type_descriptor: TypeDescriptor::plain(PluginTypeName::Optional),
        }
    }

    /// An installer whose second step only exists when the first was answered
    /// one particular way.
    fn conditional_module() -> FomodModule {
        FomodModule {
            name: "Conditional".into(),
            image: None,
            module_dependencies: None,
            required_install_files: Vec::new(),
            install_steps: vec![
                InstallStep {
                    id: "step-0".into(),
                    name: "Body".into(),
                    visible: None,
                    groups: vec![PluginGroup {
                        id: "step-0/group-0".into(),
                        name: "Shape".into(),
                        kind: GroupKind::SelectAtMostOne,
                        order: SortOrder::Explicit,
                        plugins: vec![plugin("Slim", &[("body", "slim")])],
                    }],
                },
                InstallStep {
                    id: "step-1".into(),
                    name: "Slim extras".into(),
                    visible: Some(apoc_domain::fomod::CompositeDependency::Flag {
                        name: "body".into(),
                        value: "slim".into(),
                    }),
                    groups: vec![PluginGroup {
                        id: "step-1/group-0".into(),
                        name: "Extras".into(),
                        kind: GroupKind::SelectAny,
                        order: SortOrder::Explicit,
                        plugins: vec![plugin("Belt", &[])],
                    }],
                },
            ],
            step_order: SortOrder::Explicit,
            conditional_installs: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn conditional_bundle() -> ModBundle {
        let module = conditional_module();
        let mut groups = Vec::new();
        for step in &module.install_steps {
            for group in &step.groups {
                groups.push(OptionGroup {
                    index: None,
                    label: format!("{} · {}", step.name, group.name),
                    cardinality: Some(group.kind),
                    options: group
                        .plugins
                        .iter()
                        .map(|p| ModOption {
                            id: p.id.clone(),
                            folder_name: String::new(),
                            group_index: None,
                            slot_token: None,
                            radio_set: None,
                            name: p.name.clone(),
                            description: None,
                            category: None,
                            author: None,
                            screenshot: None,
                            screenshot_archive_path: None,
                            select_mode: SelectMode::Stackable,
                            recommended: false,
                            blocked_reason: None,
                            deployable: true,
                            payload: Vec::new(),
                            raw_modinfo: Default::default(),
                        })
                        .collect(),
                });
            }
        }
        ModBundle {
            name: "Conditional".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::Fomod,
            archive_sha256: None,
            fomod: Some(module),
            groups,
        }
    }

    #[test]
    fn an_answer_that_conditions_no_longer_reach_is_dropped_rather_than_honoured() {
        // The belt was chosen last time, in a step that only exists when the
        // slim body is picked. Carrying without the body choice leaves the belt
        // unreachable, and installing it anyway would deploy files the new
        // version's own conditions say do not apply.
        let mut previous = Selection::new();
        previous.insert("belt".to_string());

        let carried = carry_selection(&conditional_bundle(), &previous);

        assert!(
            !carried.selection.contains("belt"),
            "{:?}",
            carried.selection
        );
        assert!(carried.dropped.contains(&"belt".to_string()), "{carried:?}");
        assert!(
            !carried.is_complete(),
            "an update that quietly drops a choice must open the wizard"
        );
    }

    #[test]
    fn an_answer_the_conditions_still_reach_carries_untouched() {
        let mut previous = Selection::new();
        previous.insert("slim".to_string());
        previous.insert("belt".to_string());

        let carried = carry_selection(&conditional_bundle(), &previous);

        assert!(carried.selection.contains("slim"));
        assert!(
            carried.selection.contains("belt"),
            "choosing the body again makes the extras step reachable"
        );
        assert!(carried.is_complete(), "{carried:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{InstallerModel, OptionGroup};

    fn opt(id: &str, mode: SelectMode, radio_set: Option<&str>) -> ModOption {
        ModOption {
            id: id.to_string(),
            folder_name: id.to_string(),
            group_index: None,
            slot_token: None,
            radio_set: radio_set.map(str::to_string),
            name: id.to_string(),
            description: None,
            category: None,
            author: None,
            screenshot: None,
            screenshot_archive_path: None,
            select_mode: mode,
            recommended: false,
            blocked_reason: None,
            deployable: mode != SelectMode::Info,
            payload: Vec::new(),
            raw_modinfo: Default::default(),
        }
    }

    fn bundle(options: Vec<ModOption>) -> ModBundle {
        ModBundle {
            name: "Test".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::FluffyAio,
            archive_sha256: None,
            fomod: None,
            groups: vec![OptionGroup {
                index: None,
                label: "Group".into(),
                cardinality: None,
                options,
            }],
        }
    }

    fn selection(ids: &[&str]) -> Selection {
        let mut s = Selection::new();
        for i in ids {
            s.insert((*i).to_string());
        }
        s
    }

    #[test]
    fn an_unchanged_bundle_carries_everything_and_asks_nothing() {
        let b = bundle(vec![
            opt("core", SelectMode::Forced, None),
            opt("addon-a", SelectMode::Stackable, None),
            opt("addon-b", SelectMode::Stackable, None),
        ]);
        let carried = carry_selection(&b, &selection(&["core", "addon-a"]));
        assert!(carried.selection.contains("addon-a"));
        assert!(
            !carried.selection.contains("addon-b"),
            "was not chosen before"
        );
        assert!(carried.is_complete());
    }

    #[test]
    fn forced_options_come_back_even_if_they_were_not_in_the_old_selection() {
        // A new release can promote a file to mandatory. It has to be included
        // whether or not it was chosen last time.
        let b = bundle(vec![opt("core", SelectMode::Forced, None)]);
        let carried = carry_selection(&b, &selection(&[]));
        assert!(carried.selection.contains("core"));
        assert!(carried.is_complete());
    }

    #[test]
    fn a_removed_option_is_reported_rather_than_dropped_quietly() {
        let b = bundle(vec![opt("core", SelectMode::Forced, None)]);
        let carried = carry_selection(&b, &selection(&["core", "addon-gone"]));
        assert_eq!(carried.dropped, vec!["addon-gone"]);
        assert!(!carried.is_complete(), "the result differs from last time");
    }

    #[test]
    fn a_radio_choice_that_still_exists_is_kept_and_asks_nothing() {
        let b = bundle(vec![
            opt("body-slim", SelectMode::Exclusive, Some("body")),
            opt("body-bulk", SelectMode::Exclusive, Some("body")),
        ]);
        let carried = carry_selection(&b, &selection(&["body-slim"]));
        assert!(carried.selection.contains("body-slim"));
        assert!(!carried.selection.contains("body-bulk"));
        assert!(carried.is_complete());
    }

    #[test]
    fn a_radio_set_whose_chosen_variant_vanished_needs_a_decision() {
        let b = bundle(vec![
            opt("body-bulk", SelectMode::Exclusive, Some("body")),
            opt("body-tall", SelectMode::Exclusive, Some("body")),
        ]);
        let carried = carry_selection(&b, &selection(&["body-slim"]));
        assert_eq!(carried.undecided, vec!["body"]);
        assert_eq!(carried.dropped, vec!["body-slim"]);
        assert!(!carried.is_complete());
    }

    #[test]
    fn a_radio_set_that_is_new_in_this_version_needs_a_decision() {
        // The old archive had no such choice, so nothing can carry into it.
        let b = bundle(vec![
            opt("core", SelectMode::Forced, None),
            opt("hair-long", SelectMode::Exclusive, Some("hair")),
            opt("hair-short", SelectMode::Exclusive, Some("hair")),
        ]);
        let carried = carry_selection(&b, &selection(&["core"]));
        assert_eq!(carried.undecided, vec!["hair"]);
        assert!(carried.dropped.is_empty(), "nothing was removed");
        assert!(!carried.is_complete());
    }

    #[test]
    fn each_radio_set_is_judged_on_its_own() {
        let b = bundle(vec![
            opt("body-slim", SelectMode::Exclusive, Some("body")),
            opt("body-bulk", SelectMode::Exclusive, Some("body")),
            opt("hair-long", SelectMode::Exclusive, Some("hair")),
            opt("hair-short", SelectMode::Exclusive, Some("hair")),
        ]);
        let carried = carry_selection(&b, &selection(&["body-slim"]));
        assert_eq!(
            carried.undecided,
            vec!["hair"],
            "body was answered, hair was not"
        );
    }

    #[test]
    fn an_option_demoted_to_presentation_is_not_carried() {
        // An author can turn a folder into a cover image between releases.
        // Carrying it would put a non-deployable entry into a deploy plan.
        let b = bundle(vec![opt("readme", SelectMode::Info, None)]);
        let carried = carry_selection(&b, &selection(&["readme"]));
        assert!(!carried.selection.contains("readme"));
        assert_eq!(carried.dropped, vec!["readme"]);
    }

    #[test]
    fn a_selection_from_a_completely_different_mod_carries_nothing() {
        // Guarding the case where the wrong previous selection is passed in:
        // it must not silently produce an empty-but-complete result.
        let b = bundle(vec![
            opt("core", SelectMode::Forced, None),
            opt("addon-a", SelectMode::Stackable, None),
        ]);
        let carried = carry_selection(&b, &selection(&["something-else", "and-another"]));
        assert_eq!(carried.dropped, vec!["and-another", "something-else"]);
        assert!(
            carried.selection.contains("core"),
            "forced options still apply"
        );
        assert!(!carried.is_complete());
    }

    #[test]
    fn dropped_ids_are_sorted_and_deduplicated() {
        let b = bundle(vec![opt("core", SelectMode::Forced, None)]);
        let carried = carry_selection(&b, &selection(&["zeta", "alpha"]));
        assert_eq!(carried.dropped, vec!["alpha", "zeta"]);
    }
}
