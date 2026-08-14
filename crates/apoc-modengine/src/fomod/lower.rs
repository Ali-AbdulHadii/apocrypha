//! Resolving a parsed installer against the archive it came in.
//!
//! The manifest declares sources and destinations in the author's terms; this
//! turns them into the [`FilePayload`]s the rest of the engine deploys. Two
//! translations happen here and both are where the format's real hazards live.
//!
//! **Sources are resolved case-insensitively, destinations are validated.** An
//! author writes `source="Textures\4K\a.dds"` with whatever casing their
//! filesystem tolerated, while archive lookups elsewhere match exactly. So
//! sources are matched against a lowercased index and every resulting payload
//! carries the archive's own spelling, never the manifest's.
//!
//! Destinations run the other way: this is the first format where the archive
//! dictates where a file lands, so a hostile `destination="../../.."` is a real
//! attack rather than a hypothetical one. It is rejected here, at analyze time,
//! in front of the two guards that already exist further down. Neither of those
//! is weakened; this one exists so a bad archive fails before anything is
//! written, with the offending path named.

use crate::archive::{ArchiveIndex, RawEntry};
use crate::error::{ModEngineError, Result};
use crate::rules::GameRules;
use apoc_domain::fomod::{
    FileSpec, FomodModule, GroupKind, Plugin, PluginGroup, PluginTypeName, SortOrder,
};
use apoc_domain::{
    DeployRoot, FilePayload, InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode,
};
use std::collections::BTreeMap;

/// Id of the synthetic option holding `<requiredInstallFiles>`.
pub(crate) const REQUIRED_ID: &str = "@required";

/// Turn a parsed installer plus its archive into a bundle.
pub(crate) fn lower(
    module: &FomodModule,
    index: &ArchiveIndex,
    archive_stem: &str,
    sha: Option<String>,
    rules: &GameRules,
) -> Result<ModBundle> {
    let sources = SourceIndex::build(index);
    let mut warnings = module.warnings.clone();
    let mut groups: Vec<OptionGroup> = Vec::new();

    // Files everybody gets, first, so they read as the base the rest layers on.
    if !module.required_install_files.is_empty() {
        let payload = resolve_files(
            &module.required_install_files,
            &sources,
            rules,
            &mut warnings,
        )?;
        groups.push(OptionGroup {
            index: Some(0),
            label: "Required files".to_string(),
            cardinality: Some(GroupKind::SelectAll),
            options: vec![ModOption {
                id: REQUIRED_ID.to_string(),
                folder_name: String::new(),
                group_index: Some(0),
                slot_token: None,
                radio_set: None,
                name: "Files this mod always installs".to_string(),
                description: None,
                category: None,
                author: None,
                screenshot: None,
                screenshot_archive_path: module
                    .image
                    .as_deref()
                    .and_then(|p| sources.file(p).map(|e| e.archive_path.clone())),
                select_mode: SelectMode::Forced,
                recommended: false,
                blocked_reason: None,
                deployable: !payload.is_empty(),
                payload,
                raw_modinfo: Default::default(),
            }],
        });
    }

    for step in sorted(&module.install_steps, module.step_order, |s| &s.name) {
        for group in sorted(&step.groups, SortOrder::Explicit, |g| &g.name) {
            let options = lower_group(group, &sources, rules, &mut warnings)?;
            groups.push(OptionGroup {
                index: None,
                // The author's own two levels of naming, kept together: the
                // wizard shows one page per group, and "Body" alone is not
                // enough to know which page you are on.
                label: format!("{} · {}", step.name, group.name),
                cardinality: Some(group.kind),
                options,
            });
        }
    }

    // Patterns that install files without ever offering a choice. Carried as
    // options so nothing about them is invisible; whether each applies is
    // decided by the evaluator, not by a click.
    for pattern in &module.conditional_installs {
        let payload = resolve_files(&pattern.files, &sources, rules, &mut warnings)?;
        if payload.is_empty() {
            continue;
        }
        groups.push(OptionGroup {
            index: None,
            label: "Decided by your choices".to_string(),
            cardinality: Some(GroupKind::SelectAny),
            options: vec![ModOption {
                id: pattern.id.clone(),
                folder_name: String::new(),
                group_index: None,
                slot_token: None,
                radio_set: None,
                name: "Extra files for this combination".to_string(),
                description: None,
                category: None,
                author: None,
                screenshot: None,
                screenshot_archive_path: None,
                select_mode: SelectMode::Stackable,
                recommended: false,
                blocked_reason: None,
                deployable: true,
                payload,
                raw_modinfo: Default::default(),
            }],
        });
    }

    if groups.is_empty() {
        return Err(ModEngineError::Empty);
    }

    Ok(ModBundle {
        name: if module.name.trim().is_empty() {
            archive_stem.to_string()
        } else {
            module.name.clone()
        },
        version: None,
        author: None,
        category: None,
        installer_model: InstallerModel::Fomod,
        archive_sha256: sha,
        fomod: Some(FomodModule {
            warnings: warnings.clone(),
            ..module.clone()
        }),
        groups,
    })
}

fn lower_group(
    group: &PluginGroup,
    sources: &SourceIndex<'_>,
    rules: &GameRules,
    warnings: &mut Vec<String>,
) -> Result<Vec<ModOption>> {
    let mut options = Vec::new();
    for plugin in sorted(&group.plugins, group.order, |p| &p.name) {
        options.push(lower_plugin(plugin, group, sources, rules, warnings)?);
    }
    Ok(options)
}

fn lower_plugin(
    plugin: &Plugin,
    group: &PluginGroup,
    sources: &SourceIndex<'_>,
    rules: &GameRules,
    warnings: &mut Vec<String>,
) -> Result<ModOption> {
    let payload = resolve_files(&plugin.files, sources, rules, warnings)?;
    let declared = !plugin.files.is_empty();
    let default_type = plugin.type_descriptor.default;

    // An option whose every source is missing is shown, disabled, with the
    // reason. Offering it would install nothing and look like a failure of the
    // manager rather than of the archive.
    let missing_payload = declared && payload.is_empty();
    if missing_payload {
        warnings.push(format!(
            "\"{}\" lists files that are not in the archive, so it cannot be installed.",
            plugin.name
        ));
    }

    let blocked_reason = if default_type == PluginTypeName::NotUsable {
        Some("The installer says this option cannot be used.".to_string())
    } else if missing_payload {
        Some("Its files are missing from the archive.".to_string())
    } else {
        None
    };

    let select_mode = if blocked_reason.is_some() || (!declared && payload.is_empty()) {
        SelectMode::Info
    } else {
        match (default_type, group.kind) {
            (PluginTypeName::Required, _) | (_, GroupKind::SelectAll) => SelectMode::Forced,
            (_, GroupKind::SelectExactlyOne) | (_, GroupKind::SelectAtMostOne) => {
                SelectMode::Exclusive
            }
            _ => SelectMode::Stackable,
        }
    };

    Ok(ModOption {
        id: plugin.id.clone(),
        folder_name: String::new(),
        group_index: None,
        slot_token: None,
        // Set for the two one-of cardinalities so the existing wizard renders
        // radios with no change, and the planner's own exclusivity handling
        // keeps working exactly as it does for a Fluffy radio set.
        radio_set: (select_mode == SelectMode::Exclusive).then(|| group.id.clone()),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        category: None,
        author: None,
        screenshot: plugin.image.clone(),
        screenshot_archive_path: plugin
            .image
            .as_deref()
            .and_then(|p| sources.file(p).map(|e| e.archive_path.clone())),
        select_mode,
        recommended: default_type == PluginTypeName::Recommended,
        blocked_reason,
        deployable: !payload.is_empty() && select_mode != SelectMode::Info,
        payload,
        raw_modinfo: Default::default(),
    })
}

/* --------------------------------------------------------------- files --- */

fn resolve_files(
    specs: &[FileSpec],
    sources: &SourceIndex<'_>,
    rules: &GameRules,
    warnings: &mut Vec<String>,
) -> Result<Vec<FilePayload>> {
    let mut payload: Vec<FilePayload> = Vec::new();

    for spec in specs {
        let resolved: Vec<(&RawEntry, String)> = if spec.is_folder {
            sources.folder(&spec.source)
        } else {
            match sources.file(&spec.source) {
                Some(entry) => vec![(entry, String::new())],
                None => Vec::new(),
            }
        };

        if resolved.is_empty() {
            // An author's typo, or a file they forgot to pack. Named rather
            // than silently skipped, and not fatal: the rest of the mod is
            // still installable and refusing all of it helps nobody.
            warnings.push(format!(
                "The installer asks for \"{}\", which is not in the archive.",
                spec.source
            ));
            continue;
        }

        for (entry, rel_to_source) in resolved {
            let dest = destination_for(spec, &rel_to_source, rules)?;
            payload.push(FilePayload {
                root: root_for_dest(rules, &dest),
                // The archive's own spelling. Everything downstream looks
                // entries up by exact match.
                archive_path: entry.archive_path.clone(),
                game_rel_path: dest,
                size: entry.size,
                priority: spec.priority,
            });
        }
    }

    // Ascending priority, so the vector is already in the order the planner
    // layers it: within one option, the last writer wins.
    payload.sort_by_key(|f| f.priority);
    Ok(payload)
}

/// Where one resolved entry lands, game-relative.
fn destination_for(spec: &FileSpec, rel_to_source: &str, rules: &GameRules) -> Result<String> {
    // An absent or empty destination means "the same relative path as the
    // source", which is what most single-file entries rely on.
    let declared = spec
        .destination
        .as_deref()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or(&spec.source);

    let joined = if rel_to_source.is_empty() {
        declared.to_string()
    } else if declared.is_empty() {
        rel_to_source.to_string()
    } else {
        format!("{}/{}", declared.trim_end_matches('/'), rel_to_source)
    };

    let safe = validated_dest(&joined)?;
    let prefixed = if rules.fomod_dest_prefix.is_empty() {
        safe
    } else {
        format!("{}/{}", rules.fomod_dest_prefix, safe)
    };
    // Casing last. `canonicalize` only ever swaps a whole segment for a
    // case-variant of itself, so it can introduce neither a separator nor a
    // parent reference, and validation above still holds afterwards.
    Ok(rules.canonicalize(&prefixed))
}

/// Reject a declared destination that would escape the game directory.
///
/// A hard error, never a skipped file. An installer naming `../../etc` is
/// either hostile or broken past the point where the rest of its files could be
/// trusted, and quietly dropping one while installing the others is the worst
/// available outcome.
fn validated_dest(raw: &str) -> Result<String> {
    let unsafe_path = || ModEngineError::UnsafePath(raw.to_string());
    let normalised = raw.replace('\\', "/");

    if normalised.starts_with('/') || normalised.starts_with("//") {
        return Err(unsafe_path());
    }
    // A drive letter (`C:`) or any other colon-bearing prefix.
    if normalised.contains(':') {
        return Err(unsafe_path());
    }
    if normalised.contains('\0') {
        return Err(unsafe_path());
    }

    let mut parts: Vec<&str> = Vec::new();
    for segment in normalised.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err(unsafe_path()),
            "~" => return Err(unsafe_path()),
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err(unsafe_path());
    }
    Ok(parts.join("/"))
}

fn root_for_dest(rules: &GameRules, dest: &str) -> DeployRoot {
    let first = dest.split('/').next().unwrap_or_default();
    if dest.contains('/') {
        if rules.is_payload_root(first) {
            return DeployRoot::from_root_dir(&first.to_ascii_lowercase());
        }
        return DeployRoot::Other(first.to_string());
    }
    if rules.is_pak_file(dest) {
        return DeployRoot::Pak;
    }
    DeployRoot::GameRoot
}

/* -------------------------------------------------------------- sources --- */

/// Archive entries keyed by a normalised form of their path, so a manifest's
/// casing and separators never have to match the archive's.
struct SourceIndex<'a> {
    by_key: BTreeMap<String, &'a RawEntry>,
}

impl<'a> SourceIndex<'a> {
    fn build(index: &'a ArchiveIndex) -> Self {
        let mut by_key = BTreeMap::new();
        for entry in &index.entries {
            by_key.insert(normalise(&entry.path), entry);
        }
        SourceIndex { by_key }
    }

    fn file(&self, source: &str) -> Option<&'a RawEntry> {
        self.by_key.get(&normalise(source)).copied()
    }

    /// Every entry beneath `source`, with each one's path relative to it.
    ///
    /// A range scan rather than a filter over everything: a folder entry in a
    /// large texture pack would otherwise walk the whole archive per spec.
    fn folder(&self, source: &str) -> Vec<(&'a RawEntry, String)> {
        let prefix = normalise(source);
        // An empty source means the archive root, where every entry qualifies.
        if prefix.is_empty() {
            return self.by_key.values().map(|e| (*e, e.path.clone())).collect();
        }
        let with_slash = format!("{prefix}/");
        self.by_key
            .range(with_slash.clone()..)
            .take_while(|(k, _)| k.starts_with(&with_slash))
            .map(|(k, e)| {
                let rel = k[with_slash.len()..].to_string();
                // Relative path taken from the entry's real name so the staged
                // tree keeps the author's casing, not the lowercased key.
                let real_rel = e
                    .path
                    .get(e.path.len().saturating_sub(rel.len())..)
                    .unwrap_or(&rel)
                    .to_string();
                (*e, real_rel)
            })
            .collect()
    }
}

fn normalise(path: &str) -> String {
    path.replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

/* ---------------------------------------------------------------- order --- */

/// Apply the order a list asks for, stably.
///
/// Ascending and descending sort by name; equal names keep the order the
/// document put them in, so a tie is never reshuffled between two runs of the
/// same archive.
fn sorted<'a, T, F>(items: &'a [T], order: SortOrder, key: F) -> Vec<&'a T>
where
    F: Fn(&'a T) -> &'a String,
{
    let mut out: Vec<&T> = items.iter().collect();
    match order {
        SortOrder::Explicit => {}
        SortOrder::Ascending => out.sort_by(|a, b| key(a).cmp(key(b))),
        SortOrder::Descending => out.sort_by(|a, b| key(b).cmp(key(a))),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_that_escapes_the_game_directory_is_refused() {
        for bad in [
            "../etc/passwd",
            "a/../../b",
            "/etc/passwd",
            "C:\\Windows\\system32",
            "//server/share/x",
            "~/x",
            "meshes/..",
            "",
            "..",
        ] {
            assert!(
                validated_dest(bad).is_err(),
                "{bad} should have been refused"
            );
        }
    }

    #[test]
    fn an_ordinary_destination_is_kept_with_its_casing() {
        assert_eq!(
            validated_dest("Meshes/Armor/x.nif").unwrap(),
            "Meshes/Armor/x.nif"
        );
        assert_eq!(validated_dest("./a/b.dds").unwrap(), "a/b.dds");
        assert_eq!(validated_dest("a\\b\\c.esp").unwrap(), "a/b/c.esp");
    }

    #[test]
    fn canonicalising_a_validated_path_leaves_it_validated() {
        // The safety argument for running canonicalize after validation: it
        // only ever swaps a whole segment for a case-variant of itself.
        let rules = GameRules {
            canonical_case: vec!["Data".into(), "STM".into()],
            ..GameRules::default()
        };
        let out = rules.canonicalize("data/stm/x.mesh");
        assert_eq!(out, "Data/STM/x.mesh");
        assert!(validated_dest(&out).is_ok());
    }

    #[test]
    fn a_games_prefix_is_applied_before_casing_is_forced() {
        let rules = GameRules {
            fomod_dest_prefix: "Data".into(),
            canonical_case: vec!["Data".into()],
            ..GameRules::default()
        };
        let spec = FileSpec {
            source: "meshes/x.nif".into(),
            destination: Some("meshes/x.nif".into()),
            priority: 0,
            is_folder: false,
            always_install: false,
            install_if_usable: false,
        };
        assert_eq!(
            destination_for(&spec, "", &rules).unwrap(),
            "Data/meshes/x.nif"
        );
    }

    #[test]
    fn a_file_with_no_declared_destination_mirrors_its_source() {
        let spec = FileSpec {
            source: "meshes/x.nif".into(),
            destination: None,
            priority: 0,
            is_folder: false,
            always_install: false,
            install_if_usable: false,
        };
        assert_eq!(
            destination_for(&spec, "", &GameRules::default()).unwrap(),
            "meshes/x.nif"
        );
    }
}
