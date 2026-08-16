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

/// The default selection for a bundle: every forced option, plus any the
/// installer itself recommends.
///
/// Exclusive radio sets deliberately start empty, so the wizard asks the user to
/// choose rather than silently picking a variant for them. A recommended option
/// is different in kind: the author said which one they would pick, and
/// pre-ticking their answer is not the manager inventing one. It stays freely
/// changeable, unlike a forced option.
pub fn default_selection(bundle: &ModBundle) -> Selection {
    let mut sel = Selection::new();
    for o in bundle.options() {
        if !o.deployable {
            continue;
        }
        if o.select_mode == SelectMode::Forced || o.recommended {
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
    for sibling in bundle
        .options()
        .filter(|o| o.radio_set.as_ref() == Some(set))
    {
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

    // A group whose author said it must be answered. Only manifest-driven
    // formats declare this; where cardinality is inferred from folder names the
    // field is None and nothing below applies, so an empty Fluffy radio set
    // stays what it has always been: a legitimate "keep vanilla".
    for group in &bundle.groups {
        let Some(kind) = group.cardinality else {
            continue;
        };
        if !kind.requires_answer() {
            continue;
        }
        let answerable: Vec<&ModOption> = group
            .options
            .iter()
            .filter(|o| o.select_mode != SelectMode::Info)
            .collect();
        if answerable.is_empty() {
            continue;
        }
        if !answerable.iter().any(|o| sel.contains(&o.id)) {
            issues.push(ValidationIssue {
                message: format!("Choose an option in '{}'", group.label),
                option_ids: answerable.iter().map(|o| o.id.clone()).collect(),
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
    // How strongly each destination's current winner claimed it. Layer order
    // still decides everything else; this only lets a file that says it should
    // win do so from an earlier option.
    let mut claims: HashMap<String, (i32, usize)> = HashMap::new();

    for (rank, (_, opt)) in selected.iter().enumerate() {
        for f in &opt.payload {
            let dest = f.game_rel_path.clone();
            if !contenders.contains_key(&dest) {
                order.push(dest.clone());
            }
            contenders
                .entry(dest.clone())
                .or_default()
                .push(opt.id.clone());

            // Priority first, then the layering order that was here before.
            // With priority zero everywhere, which is every option of every
            // format but FOMOD, this reduces exactly to last writer wins.
            let claim = (f.priority, rank);
            if claims.get(&dest).is_some_and(|held| *held > claim) {
                continue;
            }
            claims.insert(dest.clone(), claim);
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
        // `order` and `winners` are filled in the same pass, so a missing winner
        // cannot happen; skipping beats panicking because a planning bug should
        // cost the user one file, not the whole deploy.
        let Some(file) = winners.remove(&dest) else {
            continue;
        };
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
    /// Library identity: what conflict overrides name and what load order sorts.
    /// Stable across versions of the same mod.
    pub mod_id: String,
    /// Which directory under the game's staging root holds this mod's files.
    ///
    /// Separate from `mod_id` because a mod keeps its identity across an update
    /// while its bytes do not. Reusing one directory per mod would mean staging
    /// a new version over the old one's files, which an applied deployment is
    /// still repairing from.
    pub staging_key: String,
    /// Load-order priority; higher wins a conflict against lower.
    pub priority: i64,
    pub plan: DeploymentPlan,
}

impl ModPlan {
    /// A plan whose staging directory is named after the mod itself.
    ///
    /// The shape every mod had before staging and identity were separated, and
    /// still the right one for a caller that stages exactly once — tests, the
    /// CLI, anything with no notion of a second version.
    pub fn same_namespace(mod_id: impl Into<String>, priority: i64, plan: DeploymentPlan) -> Self {
        let mod_id = mod_id.into();
        Self {
            staging_key: mod_id.clone(),
            mod_id,
            priority,
            plan,
        }
    }
}

/// Which mod should win specific contested paths, overriding load order.
/// Key: game-relative destination path. Value: the mod id that should win.
pub type ConflictOverrides = std::collections::HashMap<String, String>;

/// Merge several mods' plans into a single deployment.
///
/// Staged paths are namespaced by staging key so one staging root serves them all,
/// and cross-mod conflicts on the same destination are resolved by load-order
/// priority: higher priority is applied last and therefore wins.
pub fn combine(mods: Vec<ModPlan>, bundle_name: &str) -> DeploymentPlan {
    combine_with_overrides(mods, bundle_name, &ConflictOverrides::new())
}

/// [`combine`], but with per-path exceptions to load order.
///
/// Reordering two mods to settle one contested file drags every other file they
/// own along with it, so a user who wants one mod's mesh and another's texture
/// cannot express that by priority alone. An override names the winner for a
/// single destination and leaves the rest of both mods where load order put them.
///
/// An override naming a mod that does not contend for that path is ignored
/// rather than an error: overrides outlive the mods they mention, and a stale
/// entry must never block a deploy.
pub fn combine_with_overrides(
    mut mods: Vec<ModPlan>,
    bundle_name: &str,
    overrides: &ConflictOverrides,
) -> DeploymentPlan {
    mods.sort_by_key(|m| m.priority);

    // Every contender's file is kept, not just the last writer, because an
    // override can promote a mod that load order would otherwise have buried.
    let mut candidates: HashMap<String, Vec<(String, PlannedFile)>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut issues = Vec::new();
    let mut conflicts = Vec::new();

    for m in &mods {
        issues.extend(m.plan.issues.iter().cloned());
        // Conflicts found within a single mod stay meaningful in the merged view.
        conflicts.extend(m.plan.conflicts.iter().cloned());

        for f in &m.plan.files {
            let dest = f.game_rel_path.clone();
            if !candidates.contains_key(&dest) {
                order.push(dest.clone());
            }
            candidates.entry(dest).or_default().push((
                m.mod_id.clone(),
                PlannedFile {
                    // Re-root the staged path into the shared staging directory.
                    // Keyed by staging key, not mod id: the id says which mod
                    // owns the file, the key says where its bytes actually are.
                    staged_rel_path: format!(
                        "{}/{}",
                        option_dir(&m.staging_key),
                        f.staged_rel_path
                    ),
                    ..f.clone()
                },
            ));
        }
    }

    let mut files = Vec::with_capacity(order.len());
    for dest in order {
        let mut c = candidates.remove(&dest).unwrap_or_default();
        if c.is_empty() {
            continue;
        }

        // Falling back to the last candidate reproduces plain load order: the
        // highest priority mod was applied last.
        let winner_at = overrides
            .get(&dest)
            .and_then(|wanted| c.iter().rposition(|(mod_id, _)| mod_id == wanted))
            .unwrap_or(c.len() - 1);
        let winner_id = c[winner_at].0.clone();

        // Only report a cross-mod conflict when distinct mods contend.
        let mut distinct: Vec<String> = Vec::new();
        for (id, _) in &c {
            if !distinct.contains(id) {
                distinct.push(id.clone());
            }
        }
        if distinct.len() > 1 {
            conflicts.push(Conflict {
                game_rel_path: dest.clone(),
                winner: winner_id,
                contenders: distinct,
            });
        }
        files.push(c.swap_remove(winner_at).1);
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
            recommended: false,
            blocked_reason: None,
            deployable: !paths.is_empty(),
            payload: paths
                .iter()
                .map(|p| FilePayload {
                    archive_path: format!("{id}/{p}"),
                    game_rel_path: p.to_string(),
                    root: DeployRoot::Natives,
                    size: 10,
                    priority: 0,
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
            fomod: None,
            unclaimed_root_files: Vec::new(),
            groups: vec![OptionGroup {
                index: Some(1),
                label: "G".into(),
                cardinality: None,
                options,
            }],
        }
    }

    #[test]
    fn a_file_that_claims_priority_wins_from_an_earlier_option() {
        // Layer order still decides everything else; priority only lets a file
        // that says it should win do so without being written last.
        let mut early = opt("early", 1, SelectMode::Stackable, None, &["natives/x"]);
        early.payload[0].priority = 9;
        let late = opt("late", 1, SelectMode::Stackable, None, &["natives/x"]);

        let b = bundle(vec![early, late]);
        let mut sel = Selection::new();
        sel.insert("early".to_string());
        sel.insert("late".to_string());

        let p = plan(&b, &sel);
        let file = p
            .files
            .iter()
            .find(|f| f.game_rel_path == "natives/x")
            .expect("planned");
        assert_eq!(file.option_id, "early");
        // The other option still shows up as a contender, so the conflict is
        // reported exactly as it was before priorities existed.
        assert_eq!(p.conflicts.len(), 1);
    }

    #[test]
    fn with_no_priorities_the_last_option_still_wins() {
        // The proof that priority zero everywhere reduces to the old rule.
        let b = bundle(vec![
            opt("first", 1, SelectMode::Stackable, None, &["natives/x"]),
            opt("second", 1, SelectMode::Stackable, None, &["natives/x"]),
        ]);
        let mut sel = Selection::new();
        sel.insert("first".to_string());
        sel.insert("second".to_string());

        let file = plan(&b, &sel)
            .files
            .into_iter()
            .find(|f| f.game_rel_path == "natives/x")
            .expect("planned");
        assert_eq!(file.option_id, "second");
    }

    #[test]
    fn a_group_the_author_said_must_be_answered_is_an_issue_until_it_is() {
        use apoc_domain::fomod::GroupKind;

        let mut b = bundle(vec![opt(
            "slim",
            1,
            SelectMode::Exclusive,
            Some("1:shape"),
            &["natives/a"],
        )]);
        b.groups[0].cardinality = Some(GroupKind::SelectExactlyOne);
        b.groups[0].label = "Body · Shape".into();

        let issues = plan(&b, &Selection::new()).issues;
        assert!(
            issues.iter().any(|i| i.message.contains("Body · Shape")),
            "{issues:?}"
        );

        let mut sel = Selection::new();
        sel.insert("slim".to_string());
        assert!(plan(&b, &sel).issues.is_empty());
    }

    #[test]
    fn a_group_with_no_declared_cardinality_may_be_left_empty() {
        // Where cardinality is inferred from folder names it stays inferred,
        // and an empty radio set remains a legitimate "keep vanilla".
        let b = bundle(vec![opt(
            "v1",
            1,
            SelectMode::Exclusive,
            Some("1:helm"),
            &["natives/a"],
        )]);
        assert!(plan(&b, &Selection::new()).issues.is_empty());
    }

    #[test]
    fn an_option_the_installer_recommends_starts_ticked_but_unlocked() {
        let mut recommended = opt(
            "slim",
            1,
            SelectMode::Exclusive,
            Some("1:shape"),
            &["natives/a"],
        );
        recommended.recommended = true;
        let b = bundle(vec![recommended]);

        let sel = default_selection(&b);
        assert!(sel.contains("slim"), "the author's own answer is offered");
        // Still exclusive rather than forced, so it can be changed.
        assert_eq!(b.groups[0].options[0].select_mode, SelectMode::Exclusive);
    }

    #[test]
    fn forced_options_are_selected_by_default() {
        let b = bundle(vec![
            opt("basic", 0, SelectMode::Forced, None, &["natives/a"]),
            opt(
                "v1",
                1,
                SelectMode::Exclusive,
                Some("1:helm"),
                &["natives/b"],
            ),
        ]);
        let sel = default_selection(&b);
        assert!(sel.contains("basic"));
        assert!(!sel.contains("v1"), "radio sets start unchosen");
    }

    #[test]
    fn choosing_a_variant_deselects_its_siblings() {
        let b = bundle(vec![
            opt(
                "v1",
                1,
                SelectMode::Exclusive,
                Some("1:helm"),
                &["natives/b"],
            ),
            opt(
                "v2",
                1,
                SelectMode::Exclusive,
                Some("1:helm"),
                &["natives/b"],
            ),
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
        let b = bundle(vec![opt(
            "basic",
            0,
            SelectMode::Forced,
            None,
            &["natives/a"],
        )]);
        let p = plan(&b, &Selection::new());
        assert!(!p.is_valid());
        assert_eq!(p.issues.len(), 1);
    }

    #[test]
    fn addon_overrides_base_on_the_same_path() {
        let b = bundle(vec![
            opt(
                "base",
                2,
                SelectMode::Exclusive,
                Some("2:body"),
                &["natives/x", "natives/y"],
            ),
            opt("addon", 2, SelectMode::Stackable, None, &["natives/x"]),
        ]);
        let mut sel = Selection::new();
        sel.insert("base");
        sel.insert("addon");
        let p = plan(&b, &sel);

        assert_eq!(p.file_count(), 2, "one entry per destination path");
        let x = p
            .files
            .iter()
            .find(|f| f.game_rel_path == "natives/x")
            .unwrap();
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
                ModPlan::same_namespace("mod-a", 0, single_plan("mod-a", "natives/a.pak")),
                ModPlan::same_namespace("mod-b", 0, single_plan("mod-b", "dinput8.dll")),
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
    fn staged_paths_follow_the_staging_key_while_conflicts_follow_the_mod_id() {
        // The single place the split can silently regress. A mod that has been
        // updated keeps its id — which is what overrides name and what the UI
        // reports — while its bytes live under a new generation.
        let updated = ModPlan {
            mod_id: "mod-a".into(),
            staging_key: "mod-a__v2".into(),
            priority: 0,
            plan: single_plan("mod-a", "natives/shared.pak"),
        };
        let other =
            ModPlan::same_namespace("mod-b", 10, single_plan("mod-b", "natives/shared.pak"));

        let mut overrides = ConflictOverrides::new();
        overrides.insert("natives/shared.pak".into(), "mod-a".into());
        let combined = combine_with_overrides(vec![updated, other], "All mods", &overrides);

        assert_eq!(combined.file_count(), 1);
        assert_eq!(
            combined.files[0].staged_rel_path, "mod-a__v2/opt/natives/shared.pak",
            "bytes come from the generation, not the identity"
        );
        assert_eq!(
            combined.conflicts[0].winner, "mod-a",
            "an override naming the mod id wins, even against higher priority"
        );
    }

    #[test]
    fn higher_priority_mod_wins_a_cross_mod_conflict() {
        let combined = combine(
            vec![
                ModPlan::same_namespace("low", 0, single_plan("low", "natives/shared.mdf2.45")),
                ModPlan::same_namespace("high", 10, single_plan("high", "natives/shared.mdf2.45")),
            ],
            "All mods",
        );

        assert_eq!(combined.file_count(), 1, "one file per destination");
        assert!(combined.files[0].staged_rel_path.starts_with("high/"));
        assert_eq!(combined.conflicts.len(), 1);
        assert_eq!(combined.conflicts[0].winner, "high");
        assert_eq!(combined.conflicts[0].contenders, vec!["low", "high"]);
    }

    fn multi_plan(mod_id: &str, dests: &[&str]) -> DeploymentPlan {
        DeploymentPlan {
            bundle_name: mod_id.into(),
            files: dests
                .iter()
                .map(|dest| PlannedFile {
                    option_id: "opt".into(),
                    staged_rel_path: format!("opt/{dest}"),
                    game_rel_path: (*dest).into(),
                    size: 1,
                })
                .collect(),
            conflicts: vec![],
            issues: vec![],
        }
    }

    fn overrides(pairs: &[(&str, &str)]) -> ConflictOverrides {
        pairs
            .iter()
            .map(|(path, mod_id)| ((*path).to_string(), (*mod_id).to_string()))
            .collect()
    }

    #[test]
    fn an_override_gives_a_contested_path_to_a_lower_priority_mod() {
        let combined = combine_with_overrides(
            vec![
                ModPlan::same_namespace("low", 0, single_plan("low", "natives/shared.mdf2.45")),
                ModPlan::same_namespace("high", 10, single_plan("high", "natives/shared.mdf2.45")),
            ],
            "All mods",
            &overrides(&[("natives/shared.mdf2.45", "low")]),
        );

        assert_eq!(combined.file_count(), 1, "one file per destination");
        assert!(
            combined.files[0].staged_rel_path.starts_with("low/"),
            "the override beats priority: {}",
            combined.files[0].staged_rel_path
        );
        assert_eq!(combined.conflicts.len(), 1);
        assert_eq!(combined.conflicts[0].winner, "low");
        assert_eq!(combined.conflicts[0].contenders, vec!["low", "high"]);
    }

    #[test]
    fn an_override_moves_only_the_path_it_names() {
        let combined = combine_with_overrides(
            vec![
                ModPlan::same_namespace(
                    "low",
                    0,
                    multi_plan("low", &["natives/body.mesh", "natives/body.tex"]),
                ),
                ModPlan::same_namespace(
                    "high",
                    10,
                    multi_plan("high", &["natives/body.mesh", "natives/body.tex"]),
                ),
            ],
            "All mods",
            &overrides(&[("natives/body.mesh", "low")]),
        );

        let staged = |dest: &str| {
            combined
                .files
                .iter()
                .find(|f| f.game_rel_path == dest)
                .map(|f| f.staged_rel_path.clone())
                .unwrap_or_default()
        };
        assert!(staged("natives/body.mesh").starts_with("low/"));
        assert!(
            staged("natives/body.tex").starts_with("high/"),
            "the sibling file still follows load order"
        );
    }

    #[test]
    fn an_override_naming_an_absent_mod_falls_back_to_load_order() {
        let combined = combine_with_overrides(
            vec![
                ModPlan::same_namespace("low", 0, single_plan("low", "natives/shared.mdf2.45")),
                ModPlan::same_namespace("high", 10, single_plan("high", "natives/shared.mdf2.45")),
            ],
            "All mods",
            &overrides(&[("natives/shared.mdf2.45", "uninstalled-mod")]),
        );

        assert_eq!(combined.file_count(), 1);
        assert!(combined.files[0].staged_rel_path.starts_with("high/"));
        assert_eq!(combined.conflicts[0].winner, "high");
    }

    #[test]
    fn an_override_on_an_uncontested_path_changes_nothing() {
        let combined = combine_with_overrides(
            vec![
                ModPlan::same_namespace("mod-a", 0, single_plan("mod-a", "natives/a.pak")),
                ModPlan::same_namespace("mod-b", 10, single_plan("mod-b", "dinput8.dll")),
            ],
            "All mods",
            &overrides(&[("natives/a.pak", "mod-b"), ("dinput8.dll", "mod-a")]),
        );

        assert_eq!(combined.file_count(), 2);
        let staged: Vec<&str> = combined
            .files
            .iter()
            .map(|f| f.staged_rel_path.as_str())
            .collect();
        assert!(staged.contains(&"mod-a/opt/natives/a.pak"));
        assert!(staged.contains(&"mod-b/opt/dinput8.dll"));
        assert!(combined.conflicts.is_empty(), "no path is contested");
    }

    #[test]
    fn the_reported_conflict_winner_matches_the_file_that_was_planned() {
        let dest = "natives/three-way.tex";
        let combined = combine_with_overrides(
            vec![
                ModPlan::same_namespace("bottom", 0, single_plan("bottom", dest)),
                ModPlan::same_namespace("middle", 5, single_plan("middle", dest)),
                ModPlan::same_namespace("top", 10, single_plan("top", dest)),
            ],
            "All mods",
            &overrides(&[(dest, "middle")]),
        );

        assert_eq!(combined.file_count(), 1);
        assert_eq!(combined.conflicts.len(), 1);
        let winner = &combined.conflicts[0].winner;
        assert_eq!(winner, "middle");
        assert!(
            combined.files[0]
                .staged_rel_path
                .starts_with(&format!("{winner}/")),
            "the UI's reported winner is the file on disk: {}",
            combined.files[0].staged_rel_path
        );
        assert_eq!(
            combined.conflicts[0].contenders,
            vec!["bottom", "middle", "top"],
            "every contender stays visible"
        );
    }

    #[test]
    fn staged_paths_are_namespaced_per_option() {
        assert_eq!(
            staged_rel_path("Helm-01", "natives/a.mesh.1"),
            "Helm-01/natives/a.mesh.1"
        );
    }
}
