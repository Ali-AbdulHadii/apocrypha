//! Turn a raw [`ArchiveIndex`] into a canonical [`ModBundle`]: strip a redundant
//! wrapper folder, classify the installer model, and derive option groups/roles.

use crate::archive::ArchiveIndex;
use crate::modinfo::Modinfo;
use crate::naming;
use crate::rules::GameRules;
use apoc_domain::{
    DeployRoot, FilePayload, InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode,
};
use std::collections::HashMap;

fn first_component(path: &str) -> &str {
    match path.find('/') {
        Some(idx) => &path[..idx],
        None => path,
    }
}

/// Strip a single redundant wrapper directory (e.g. `Ver.R Hirabami FM AIO v1.03/`)
/// shared by every entry, repeating while the top level is exactly one directory
/// that is neither a payload root nor a mod folder (holding `modinfo.ini`).
fn strip_wrapper(index: &mut ArchiveIndex, rules: &GameRules) {
    let mut prefix = String::new();

    loop {
        let mut firsts = std::collections::BTreeSet::new();
        let mut file_at_root = false;

        for e in &index.entries {
            let rest = &e.path[prefix.len()..];
            if rest.is_empty() {
                continue;
            }
            match rest.find('/') {
                Some(idx) => {
                    firsts.insert(rest[..idx].to_string());
                }
                None => {
                    file_at_root = true;
                    firsts.insert(rest.to_string());
                }
            }
        }

        if firsts.len() != 1 || file_at_root {
            break;
        }
        let only = firsts.into_iter().next().unwrap();
        // Never strip a directory that IS the payload, or one that only needs a
        // prefix restored above it (`autorun/`, `STM/`). Stripping those turns a
        // valid mod into an empty one.
        if rules.is_payload_root(&only) || rules.rewrap_prefix(&only).is_some() {
            break;
        }
        let candidate = format!("{prefix}{only}/");
        let has_direct_modinfo = index.entries.iter().any(|e| {
            e.path
                .eq_ignore_ascii_case(&format!("{candidate}modinfo.ini"))
        });
        if has_direct_modinfo {
            break;
        }
        prefix = candidate;
    }

    if prefix.is_empty() {
        return;
    }

    for e in &mut index.entries {
        if let Some(stripped) = e.path.strip_prefix(&prefix) {
            e.path = stripped.to_string();
        }
    }
    for (dir, _) in &mut index.modinfos {
        if let Some(stripped) = dir.strip_prefix(&prefix) {
            *dir = stripped.to_string();
        }
    }
}

/// Classify the archive into an installer model from its shape.
fn detect(index: &ArchiveIndex, rules: &GameRules) -> InstallerModel {
    match index.modinfos.len() {
        n if n >= 2 => InstallerModel::FluffyAio,
        1 => InstallerModel::FluffySingle,
        _ => {
            let has_natives = index
                .entries
                .iter()
                .any(|e| first_component(&e.path).eq_ignore_ascii_case("natives"));
            let has_ref = index
                .entries
                .iter()
                .any(|e| first_component(&e.path).eq_ignore_ascii_case("reframework"));
            // Any root this game declares as payload, or one the rewrap rules
            // can restore a prefix above. Asked of the profile rather than of a
            // fixed list, so a game with different roots is still recognized
            // instead of being reported as unclassifiable.
            let has_declared_root = index.entries.iter().any(|e| {
                let root = first_component(&e.path);
                e.path.contains('/')
                    && (rules.is_payload_root(root) || rules.rewrap_prefix(root).is_some())
            });
            // A bare loader DLL at the archive root, e.g. REFramework's release zip.
            let has_loader = index
                .entries
                .iter()
                .any(|e| !e.path.contains('/') && rules.is_root_file(&e.path));

            if has_natives {
                InstallerModel::FlatNatives
            } else if has_ref {
                InstallerModel::ReframeworkOnly
            } else if has_declared_root {
                InstallerModel::LooseRoots
            } else if has_loader {
                InstallerModel::Loader
            } else {
                InstallerModel::Unknown
            }
        }
    }
}

/// Collect the deployable payload for files living under `folder` (or, if
/// `folder` is empty, at the archive root).
fn payload_for(index: &ArchiveIndex, folder: &str, rules: &GameRules) -> Vec<FilePayload> {
    let mut out = Vec::new();
    for e in &index.entries {
        let rest = if folder.is_empty() {
            e.path.as_str()
        } else {
            match e.path.strip_prefix(&format!("{folder}/")) {
                Some(r) => r,
                None => continue,
            }
        };

        // A declared loader file sitting directly at the root deploys to the
        // path the game profile gives it, not under a payload folder. That is
        // usually the game directory itself, but RED4ext's proxy belongs in
        // `bin/x64/`, and dropping it at the root would simply never load.
        if !rest.contains('/') {
            if let Some(dest) = rules.root_file_dest(rest) {
                out.push(FilePayload {
                    archive_path: e.archive_path.clone(),
                    game_rel_path: dest.to_string(),
                    root: DeployRoot::GameRoot,
                    size: e.size,
                });
                continue;
            }
        }

        // A standalone `.pak` is the whole mod; it is renamed into the engine's
        // patch chain at deploy time, so its final name is not decided here.
        if !rest.contains('/') && rules.is_pak_file(rest) {
            out.push(FilePayload {
                archive_path: e.archive_path.clone(),
                game_rel_path: rest.to_string(),
                root: DeployRoot::Pak,
                size: e.size,
            });
            continue;
        }

        let root_name = first_component(rest);

        if rules.is_payload_root(root_name) {
            out.push(FilePayload {
                // The true in-archive name, so extraction can find this entry.
                archive_path: e.archive_path.clone(),
                game_rel_path: rules.canonicalize(rest),
                root: DeployRoot::from_root_dir(&root_name.to_ascii_lowercase()),
                size: e.size,
            });
            continue;
        }

        // The archive was packed from the inside out: restore the prefix the
        // author dropped, e.g. a bare `autorun/` becomes `reframework/autorun/`.
        if let Some(prefix) = rules.rewrap_prefix(root_name) {
            let rewrapped = format!("{prefix}/{rest}");
            out.push(FilePayload {
                archive_path: e.archive_path.clone(),
                game_rel_path: rules.canonicalize(&rewrapped),
                root: DeployRoot::from_root_dir(&prefix.to_ascii_lowercase()),
                size: e.size,
            });
        }
    }
    out
}

/// Decide how an option participates in the wizard, from its slot token, whether
/// it carries a payload, and its `modinfo` text. Payload presence and description
/// keywords are authoritative, never the folder name alone.
fn classify(deployable: bool, slot: Option<&str>, text_lower: &str, is_dummy: bool) -> SelectMode {
    // Fluffy's `DummyMod=True` marks a menu organiser entry that ships no files
    // by design. Treating it as installable is what produces a "0 files" install.
    if is_dummy {
        return SelectMode::Info;
    }
    if let Some(s) = slot {
        if s.starts_with("addon") {
            return SelectMode::Stackable;
        }
    }
    if !deployable {
        return SelectMode::Info;
    }
    if text_lower.contains("must install") {
        return SelectMode::Forced;
    }
    if text_lower.contains("select only one") {
        return SelectMode::Exclusive;
    }
    match slot {
        Some(s) if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) => SelectMode::Exclusive,
        _ => SelectMode::Stackable,
    }
}

/// Resolve the `screenshot=` filename to a real entry inside the archive.
/// Matching is case-insensitive because `modinfo.ini` casing is unreliable.
fn resolve_screenshot(index: &ArchiveIndex, folder: &str, screenshot: &str) -> Option<String> {
    if screenshot.is_empty() {
        return None;
    }
    let wanted = if folder.is_empty() {
        screenshot.to_string()
    } else {
        format!("{folder}/{screenshot}")
    };
    index
        .entries
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(&wanted))
        .map(|e| e.archive_path.clone())
}

fn build_option(
    index: &ArchiveIndex,
    folder: &str,
    raw_modinfo: &str,
    rules: &GameRules,
) -> ModOption {
    let mi = Modinfo::parse(raw_modinfo);
    let n = naming::parse_folder(folder);
    let payload = payload_for(index, folder, rules);
    let deployable = !payload.is_empty();

    let display_name = mi
        .name()
        .map(str::to_string)
        .unwrap_or_else(|| folder.to_string());
    let description = mi.description();
    let text_lower = format!(
        "{} {}",
        display_name,
        description.clone().unwrap_or_default()
    )
    .to_ascii_lowercase();

    let is_dummy = mi
        .get("DummyMod")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let select_mode = classify(deployable, n.slot_token.as_deref(), &text_lower, is_dummy);
    let radio_set = if select_mode == SelectMode::Exclusive {
        Some(naming::radio_set_key(n.group_index, &n.body))
    } else {
        None
    };

    ModOption {
        id: folder.to_string(),
        folder_name: folder.to_string(),
        group_index: n.group_index,
        slot_token: n.slot_token,
        radio_set,
        name: display_name,
        description,
        category: mi.category().map(str::to_string),
        author: mi.author().map(str::to_string),
        screenshot: mi.screenshot().map(str::to_string),
        screenshot_archive_path: mi
            .screenshot()
            .and_then(|s| resolve_screenshot(index, folder, s)),
        select_mode,
        deployable,
        payload,
        raw_modinfo: mi.raw,
    }
}

/// Group options by their `-N-` index, preserving first-seen order, and label
/// each group from the first option that carries a label word.
fn assemble_groups(options: Vec<ModOption>) -> Vec<OptionGroup> {
    let mut order: Vec<Option<u32>> = Vec::new();
    let mut buckets: HashMap<Option<u32>, Vec<ModOption>> = HashMap::new();

    for opt in options {
        let key = opt.group_index;
        if !buckets.contains_key(&key) {
            order.push(key);
        }
        buckets.entry(key).or_default().push(opt);
    }

    order
        .into_iter()
        .map(|key| {
            let opts = buckets.remove(&key).unwrap();
            let label = opts
                .iter()
                .find_map(|o| naming::parse_folder(&o.folder_name).label_word)
                .unwrap_or_else(|| match key {
                    Some(0) => "Basic".to_string(),
                    Some(i) => format!("Group {i}"),
                    None => "Options".to_string(),
                });
            OptionGroup {
                index: key,
                label,
                options: opts,
            }
        })
        .collect()
}

fn most_common_bundle_name(index: &ArchiveIndex) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, raw) in &index.modinfos {
        if let Some(nab) = Modinfo::parse(raw).name_as_bundle() {
            *counts.entry(nab.to_string()).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, c)| *c).map(|(n, _)| n)
}

/// Build a `ModBundle` from a Fluffy segmented (AIO) archive.
fn normalize_fluffy_aio(
    index: &ArchiveIndex,
    archive_stem: &str,
    sha: Option<String>,
    rules: &GameRules,
) -> ModBundle {
    let options: Vec<ModOption> = index
        .modinfos
        .iter()
        .map(|(folder, raw)| build_option(index, folder, raw, rules))
        .collect();

    // Bundle-level metadata: prefer the forced/basic option, else the first.
    let meta_src = options
        .iter()
        .find(|o| o.select_mode == SelectMode::Forced)
        .or_else(|| options.first());

    let name = most_common_bundle_name(index)
        .or_else(|| meta_src.map(|o| o.name.clone()))
        .unwrap_or_else(|| archive_stem.to_string());

    let version = meta_src.and_then(|o| o.raw_modinfo_get("version"));
    let author = options.iter().find_map(|o| o.author.clone());
    let category = options.iter().find_map(|o| o.category.clone());

    ModBundle {
        name,
        version,
        author,
        category,
        installer_model: InstallerModel::FluffyAio,
        archive_sha256: sha,
        groups: assemble_groups(options),
    }
}

/// Build a `ModBundle` for a single-option archive (loose dump or single Fluffy mod).
fn normalize_single(
    index: &ArchiveIndex,
    model: InstallerModel,
    archive_stem: &str,
    sha: Option<String>,
    rules: &GameRules,
) -> ModBundle {
    let (folder, raw) = match index.modinfos.first() {
        Some((f, r)) => (f.clone(), Some(r.clone())),
        None => (String::new(), None),
    };

    let option = match &raw {
        Some(raw) => {
            let mut o = build_option(index, &folder, raw, rules);
            // A single mod is effectively forced (all-or-nothing) if it deploys.
            if o.deployable {
                o.select_mode = SelectMode::Forced;
            }
            o
        }
        None => {
            let payload = payload_for(index, &folder, rules);
            let deployable = !payload.is_empty();
            ModOption {
                id: "main".to_string(),
                folder_name: folder.clone(),
                group_index: None,
                slot_token: None,
                radio_set: None,
                name: archive_stem.to_string(),
                description: None,
                category: None,
                author: None,
                screenshot: None,
                screenshot_archive_path: None,
                select_mode: if deployable {
                    SelectMode::Forced
                } else {
                    SelectMode::Info
                },
                deployable,
                payload,
                raw_modinfo: Default::default(),
            }
        }
    };

    let name = Modinfo::parse(raw.as_deref().unwrap_or(""))
        .name_as_bundle()
        .map(str::to_string)
        .unwrap_or_else(|| {
            if option.name.is_empty() {
                archive_stem.to_string()
            } else {
                option.name.clone()
            }
        });
    let version = option.raw_modinfo_get("version");
    let author = option.author.clone();
    let category = option.category.clone();

    ModBundle {
        name,
        version,
        author,
        category,
        installer_model: model,
        archive_sha256: sha,
        groups: vec![OptionGroup {
            index: None,
            label: "Main".to_string(),
            options: vec![option],
        }],
    }
}

/// Normalize a fully-read archive index into a canonical bundle.
pub fn normalize(
    mut index: ArchiveIndex,
    archive_stem: &str,
    sha: Option<String>,
    rules: &GameRules,
) -> ModBundle {
    strip_wrapper(&mut index, rules);
    let model = detect(&index, rules);
    match model {
        InstallerModel::FluffyAio => normalize_fluffy_aio(&index, archive_stem, sha, rules),
        other => normalize_single(&index, other, archive_stem, sha, rules),
    }
}

/// Small helper trait so bundle metadata can read raw modinfo values.
trait RawModinfoGet {
    fn raw_modinfo_get(&self, key: &str) -> Option<String>;
}
impl RawModinfoGet for ModOption {
    fn raw_modinfo_get(&self, key: &str) -> Option<String> {
        self.raw_modinfo
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
    }
}
