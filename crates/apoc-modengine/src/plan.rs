//! Selection handling and deployment planning.
//!
//! Turns a user's wizard [`Selection`] over a [`ModBundle`] into an explicit,
//! conflict-resolved [`DeploymentPlan`]. Layering follows the brief: options are
//! applied in group order, and within a group base variants are applied before
//! stackable addons, so an addon overriding a base file wins (last writer wins).

use apoc_domain::{
    Conflict, DeploymentPlan, ModBundle, ModOption, PlannedFile, SelectMode, Selection,
    ValidationIssue,
};
use std::collections::HashMap;

/// Sanitize an option id into a single safe path segment for the staging layout.
pub fn option_dir(option_id: &str) -> String {
    option_id
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c => c,
        })
        .collect()
}

/// The staged path for a payload file: `<option_dir>/<game_rel_path>`.
pub fn staged_rel_path(option_id: &str, game_rel_path: &str) -> String {
    format!("{}/{}", option_dir(option_id), game_rel_path)
}

/// The default selection for a bundle: every forced option, and nothing else.
///
/// Exclusive radio sets deliberately start empty, so the wizard asks the user to
/// choose rather than silently picking a variant for them.
pub fn default_selection(bundle: &ModBundle) -> Selection {
    let mut sel = Selection::new();
    for o in bundle.options() {
        if o.select_mode == SelectMode::Forced && o.deployable {
            sel.insert(o.id.clone());
        }
    }
    sel
}

/// Convenience for tests/CLI: forced options plus the first variant of every
/// radio set, giving a complete and immediately installable selection.
pub fn recommended_selection(bundle: &ModBundle) -> Selection {
    let mut sel = default_selection(bundle);
    let mut seen_sets: Vec<String> = Vec::new();
    for o in bundle.options() {
        if let Some(set) = &o.radio_set {
            if !seen_sets.contains(set) {
                seen_sets.push(set.clone());
                sel.insert(o.id.clone());
            }
        }
    }
    sel
}

/// Apply a radio choice: select `option_id` and deselect every sibling sharing
/// its radio set. A no-op if the option is not exclusive.
pub fn choose_exclusive(bundle: &ModBundle, sel: &mut Selection, option_id: &str) {
    let Some(target) = bundle.options().find(|o| o.id == option_id) else {
        return;
    };
    let Some(set) = &target.radio_set else {
        return;
    };
    for sibling in bundle.options().filter(|o| o.radio_set.as_ref() == Some(set)) {
        sel.remove(&sibling.id);
    }
    sel.insert(option_id.to_string());
}

/// Toggle a stackable addon (or any non-exclusive option) on/off.
pub fn toggle(bundle: &ModBundle, sel: &mut Selection, option_id: &str) {
    let Some(target) = bundle.options().find(|o| o.id == option_id) else {
        return;
    };
    match target.select_mode {
        SelectMode::Exclusive => choose_exclusive(bundle, sel, option_id),
        SelectMode::Forced | SelectMode::Info => {} // not user-toggleable
        SelectMode::Stackable => {
            if sel.contains(option_id) {
                sel.remove(option_id);
            } else {
                sel.insert(option_id.to_string());
            }
        }
    }
}

/// Validate a selection: forced options present, no radio set with >1 choice.
///
/// An *empty* radio set is not an error: "none / keep vanilla" is a legitimate
/// choice for optional parts, but the UI surfaces it as an unmade choice.
fn validate(bundle: &ModBundle, sel: &Selection) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    for o in bundle.options() {
        if o.select_mode == SelectMode::Forced && o.deployable && !sel.contains(&o.id) {
            issues.push(ValidationIssue {
                message: format!("Required option '{}' must be installed", o.name),
                option_ids: vec![o.id.clone()],
            });
        }
        if o.select_mode == SelectMode::Info && sel.contains(&o.id) {
            issues.push(ValidationIssue {
                message: format!("'{}' is informational and cannot be installed", o.name),
                option_ids: vec![o.id.clone()],
            });
        }
    }

    let mut per_set: HashMap<&str, Vec<&ModOption>> = HashMap::new();
    for o in bundle.options() {
        if let Some(set) = &o.radio_set {
            if sel.contains(&o.id) {
                per_set.entry(set.as_str()).or_default().push(o);
            }
        }
    }
    for (set, chosen) in per_set {
        if chosen.len() > 1 {
            issues.push(ValidationIssue {
                message: format!(
                    "Only one option may be selected in '{}' ({} selected)",
                    set.split_once(':').map(|x| x.1).unwrap_or(set),
                    chosen.len()
                ),
                option_ids: chosen.iter().map(|o| o.id.clone()).collect(),
            });
        }
    }

    issues
}

/// Layering order for a selected option: group index, then base-before-addon,
/// then original order. Later entries override earlier ones on the same path.
fn layer_rank(option: &ModOption, ordinal: usize) -> (u32, u8, usize) {
    let group = option.group_index.unwrap_or(u32::MAX);
    let tier = match option.select_mode {
        SelectMode::Forced => 0,
        SelectMode::Exclusive => 1,
        SelectMode::Stackable => 2, // addons layer over the base variant
        SelectMode::Info => 3,
    };
    (group, tier, ordinal)
}

/// Resolve a selection into a deployment plan.
pub fn plan(bundle: &ModBundle, sel: &Selection) -> DeploymentPlan {
    let issues = validate(bundle, sel);

    let mut selected: Vec<(usize, &ModOption)> = bundle
        .options()
        .enumerate()
        .filter(|(_, o)| o.deployable && sel.contains(&o.id))
        .collect();
    selected.sort_by_key(|(i, o)| layer_rank(o, *i));

    // Destination path -> (winning file, contenders in order).
    let mut winners: HashMap<String, PlannedFile> = HashMap::new();
    let mut contenders: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for (_, opt) in &selected {
        for f in &opt.payload {
            let dest = f.game_rel_path.clone();
            if !contenders.contains_key(&dest) {
                order.push(dest.clone());
            }
            contenders.entry(dest.clone()).or_default().push(opt.id.clone());
            winners.insert(
                dest.clone(),
                PlannedFile {
                    option_id: opt.id.clone(),
                    staged_rel_path: staged_rel_path(&opt.id, &f.game_rel_path),
                    game_rel_path: dest,
                    size: f.size,
                },
            );
        }
    }

    let mut files = Vec::with_capacity(order.len());
    let mut conflicts = Vec::new();
    for dest in order {
        let file = winners.remove(&dest).expect("winner recorded for every path");
        let c = contenders.remove(&dest).unwrap_or_default();
        if c.len() > 1 {
            conflicts.push(Conflict {
                game_rel_path: dest.clone(),
                winner: file.option_id.clone(),
                contenders: c,
            });
        }
        files.push(file);
    }

    DeploymentPlan {
        bundle_name: bundle.name.clone(),
        files,
        conflicts,
        issues,
    }
}

/// One mod's contribution to a combined deployment.
pub struct ModPlan {
    /// Staging namespace for this mod (its directory under the game's staging root).
    pub mod_id: String,
    /// Load-order priority; higher wins a conflict against lower.
    pub priority: i64,
    pub plan: DeploymentPlan,
}

/// Merge several mods' plans into a single deployment.
///
/// Staged paths are namespaced by mod id so one staging root serves them all,
/// and cross-mod conflicts on the same destination are resolved by load-order
/// priority: higher priority is applied last and therefore wins.
pub fn combine(mut mods: Vec<ModPlan>, bundle_name: &str) -> DeploymentPlan {
    mods.sort_by_key(|m| m.priority);

    let mut winners: HashMap<String, PlannedFile> = HashMap::new();
    let mut contenders: HashMap<String, Vec<String>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut issues = Vec::new();
    let mut conflicts = Vec::new();

    for m in &mods {
        issues.extend(m.plan.issues.iter().cloned());
        // Conflicts found within a single mod stay meaningful in the merged view.
        conflicts.extend(m.plan.conflicts.iter().cloned());

        for f in &m.plan.files {
            let dest = f.game_rel_path.clone();
            if !contenders.contains_key(&dest) {
                order.push(dest.clone());
            }
            contenders
                .entry(dest.clone())
                .or_default()
                .push(m.mod_id.clone());
            winners.insert(
                dest.clone(),
                PlannedFile {
                    // Re-root the staged path into the shared staging directory.
                    staged_rel_path: format!("{}/{}", option_dir(&m.mod_id), f.staged_rel_path),
                    ..f.clone()
                },
            );
        }
    }

    let mut files = Vec::with_capacity(order.len());
    for dest in order {
        let file = winners.remove(&dest).expect("winner recorded");
        let c = contenders.remove(&dest).unwrap_or_default();
        // Only report a cross-mod conflict when distinct mods contend.
        let mut distinct: Vec<String> = Vec::new();
        for id in &c {
            if !distinct.contains(id) {
                distinct.push(id.clone());
            }
        }
        if distinct.len() > 1 {
            conflicts.push(Conflict {
                game_rel_path: dest.clone(),
                winner: distinct.last().cloned().unwrap_or_default(),
                contenders: distinct,
            });
        }
        files.push(file);
    }

    DeploymentPlan {
        bundle_name: bundle_name.to_string(),
        files,
        conflicts,
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apoc_domain::{DeployRoot, FilePayload, ModBundle, ModOption, OptionGroup};
    use std::collections::BTreeMap;

    fn opt(
        id: &str,
        group: u32,
        mode: SelectMode,
        radio: Option<&str>,
        paths: &[&str],
    ) -> ModOption {
        ModOption {
            id: id.to_string(),
            folder_name: id.to_string(),
            group_index: Some(group),
            slot_token: None,
            radio_set: radio.map(str::to_string),
            name: id.to_string(),
            description: None,
            category: None,
            author: None,
            screenshot: None,
            screenshot_archive_path: None,
            select_mode: mode,
            deployable: !paths.is_empty(),
            payload: paths
                .iter()
                .map(|p| FilePayload {
                    archive_path: format!("{id}/{p}"),
                    game_rel_path: p.to_string(),
                    root: DeployRoot::Natives,
                    size: 10,
                })
                .collect(),
            raw_modinfo: BTreeMap::new(),
        }
    }

    fn bundle(options: Vec<ModOption>) -> ModBundle {
        ModBundle {
            name: "Test".into(),
            version: None,
            author: None,
            category: None,
            installer_model: apoc_domain::InstallerModel::FluffyAio,
            archive_sha256: None,
            groups: vec![OptionGroup {
                index: Some(1),
                label: "G".into(),
                options,
            }],
        }
    }

    #[test]
    fn forced_options_are_selected_by_default() {
        let b = bundle(vec![
            opt("basic", 0, SelectMode::Forced, None, &["natives/a"]),
            opt("v1", 1, SelectMode::Exclusive, Some("1:helm"), &["natives/b"]),
        ]);
        let sel = default_selection(&b);
        assert!(sel.contains("basic"));
        assert!(!sel.contains("v1"), "radio sets start unchosen");
    }

    #[test]
    fn choosing_a_variant_deselects_its_siblings() {
        let b = bundle(vec![
            opt("v1", 1, SelectMode::Exclusive, Some("1:helm"), &["natives/b"]),
            opt("v2", 1, SelectMode::Exclusive, Some("1:helm"), &["natives/b"]),
        ]);
        let mut sel = Selection::new();
        choose_exclusive(&b, &mut sel, "v1");
        choose_exclusive(&b, &mut sel, "v2");
        assert!(!sel.contains("v1"));
        assert!(sel.contains("v2"));
        assert_eq!(sel.len(), 1);
    }

    #[test]
    fn missing_forced_option_is_an_issue() {
        let b = bundle(vec![opt("basic", 0, SelectMode::Forced, None, &["natives/a"])]);
        let p = plan(&b, &Selection::new());
        assert!(!p.is_valid());
        assert_eq!(p.issues.len(), 1);
    }

    #[test]
    fn addon_overrides_base_on_the_same_path() {
        let b = bundle(vec![
            opt("base", 2, SelectMode::Exclusive, Some("2:body"), &["natives/x", "natives/y"]),
            opt("addon", 2, SelectMode::Stackable, None, &["natives/x"]),
        ]);
        let mut sel = Selection::new();
        sel.insert("base");
        sel.insert("addon");
        let p = plan(&b, &sel);

        assert_eq!(p.file_count(), 2, "one entry per destination path");
        let x = p.files.iter().find(|f| f.game_rel_path == "natives/x").unwrap();
        assert_eq!(x.option_id, "addon", "addon layers over the base variant");
        assert_eq!(p.conflicts.len(), 1);
        assert_eq!(p.conflicts[0].contenders, vec!["base", "addon"]);
    }

    #[test]
    fn info_options_cannot_be_installed() {
        let mut info = opt("warning", 0, SelectMode::Info, None, &[]);
        info.deployable = false;
        let b = bundle(vec![info]);
        let mut sel = Selection::new();
        sel.insert("warning");
        let p = plan(&b, &sel);
        assert!(p.issues.iter().any(|i| i.message.contains("informational")));
    }

    fn single_plan(mod_id: &str, dest: &str) -> DeploymentPlan {
        DeploymentPlan {
            bundle_name: mod_id.into(),
            files: vec![PlannedFile {
                option_id: "opt".into(),
                staged_rel_path: format!("opt/{dest}"),
                game_rel_path: dest.into(),
                size: 1,
            }],
            conflicts: vec![],
            issues: vec![],
        }
    }

    #[test]
    fn combining_mods_namespaces_staging_and_keeps_every_file() {
        let combined = combine(
            vec![
                ModPlan {
                    mod_id: "mod-a".into(),
                    priority: 0,
                    plan: single_plan("mod-a", "natives/a.pak"),
                },
                ModPlan {
                    mod_id: "mod-b".into(),
                    priority: 0,
                    plan: single_plan("mod-b", "dinput8.dll"),
                },
            ],
            "All mods",
        );

        assert_eq!(combined.file_count(), 2, "every mod contributes");
        let staged: Vec<&str> = combined
            .files
            .iter()
            .map(|f| f.staged_rel_path.as_str())
            .collect();
        assert!(staged.contains(&"mod-a/opt/natives/a.pak"));
        assert!(staged.contains(&"mod-b/opt/dinput8.dll"));
        assert!(combined.is_valid());
    }

    #[test]
    fn higher_priority_mod_wins_a_cross_mod_conflict() {
        let combined = combine(
            vec![
                ModPlan {
                    mod_id: "low".into(),
                    priority: 0,
                    plan: single_plan("low", "natives/shared.mdf2.45"),
                },
                ModPlan {
                    mod_id: "high".into(),
                    priority: 10,
                    plan: single_plan("high", "natives/shared.mdf2.45"),
                },
            ],
            "All mods",
        );

        assert_eq!(combined.file_count(), 1, "one file per destination");
        assert!(combined.files[0].staged_rel_path.starts_with("high/"));
        assert_eq!(combined.conflicts.len(), 1);
        assert_eq!(combined.conflicts[0].winner, "high");
        assert_eq!(combined.conflicts[0].contenders, vec!["low", "high"]);
    }

    #[test]
    fn staged_paths_are_namespaced_per_option() {
        assert_eq!(staged_rel_path("Helm-01", "natives/a.mesh.1"), "Helm-01/natives/a.mesh.1");
    }
}
