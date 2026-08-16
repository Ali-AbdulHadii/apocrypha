//! The canonical internal representation of a mod, normalized from any archive
//! format (Fluffy AIO, FOMOD, flat `natives/` dump, ...).
//!
//! The hierarchy is: `ModBundle` -> `OptionGroup` -> `ModOption` -> `FilePayload`.
//! A Fluffy "AIO" collapses to one bundle (keyed by `NameAsBundle`) whose groups
//! come from the `-N-` folder tokens and whose per-option select mode is derived
//! from the slot token and payload presence.

use crate::fomod::{FomodModule, GroupKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How the archive was recognized. Drives which normalizer produced the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerModel {
    /// Fluffy multi-option segmented installer (>=2 option folders, shared bundle).
    FluffyAio,
    /// Single Fluffy mod (one `modinfo.ini`, one payload).
    FluffySingle,
    /// Flat loose-file dump: `natives/` (and/or `reframework/`), no `modinfo.ini`.
    FlatNatives,
    /// REFramework-only mod (`reframework/`, no `natives/`).
    ReframeworkOnly,
    /// Loose files under payload roots this game declares, with no installer
    /// metadata: a Cyberpunk mod shipping `archive/`, `r6/` and `red4ext/`.
    LooseRoots,
    /// A mod loader distributed as a bare proxy DLL at the archive root
    /// (e.g. REFramework's `dinput8.dll`), with no payload folders.
    Loader,
    /// A FOMOD: `fomod/ModuleConfig.xml` declares the options, their conditions
    /// and where each file goes. The only model whose structure is stated by the
    /// author rather than inferred from the archive's shape.
    Fomod,
    /// Could not be classified; requires manual mapping (never auto-deployed blindly).
    Unknown,
}

/// How an option participates in the install wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelectMode {
    /// Must be installed; pre-checked and locked (e.g. "-0- basic files (must install)").
    Forced,
    /// One-of-many within its group (radio). Descriptions often say "select only one".
    Exclusive,
    /// Optional, independently toggleable (checkbox). Typically "addon-NN".
    Stackable,
    /// Presentation-only (cover/warning/segment header). Never deployed.
    Info,
}

/// Which top-level payload root a file belongs to; maps to a game deploy target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeployRoot {
    /// RE Engine loose files under `natives/...`.
    Natives,
    /// REFramework data/scripts under `reframework/...`.
    Reframework,
    /// A file that lands directly in the game directory (loader proxy DLLs).
    GameRoot,
    /// A standalone `.pak` archive that must be renamed into the engine's patch
    /// chain at deploy time; its final filename is not known until then.
    Pak,
    /// A root the game profile does not (yet) know how to deploy.
    Other(String),
}

impl DeployRoot {
    /// Classify an archive top-level directory name into a payload root.
    pub fn from_root_dir(name: &str) -> Self {
        match name {
            "natives" => DeployRoot::Natives,
            "reframework" => DeployRoot::Reframework,
            other => DeployRoot::Other(other.to_string()),
        }
    }

    pub fn root_dir(&self) -> &str {
        match self {
            DeployRoot::Natives => "natives",
            DeployRoot::Reframework => "reframework",
            DeployRoot::GameRoot | DeployRoot::Pak => "",
            DeployRoot::Other(s) => s.as_str(),
        }
    }
}

/// A single file to be deployed, with paths preserved verbatim (casing intact,
/// versioned RE Engine extensions like `.mesh.241111606` never normalized).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePayload {
    /// Full path of this entry inside the archive.
    pub archive_path: String,
    /// Path relative to the game install root, e.g. `natives/stm/art/.../x.mesh.241111606`.
    pub game_rel_path: String,
    /// Which payload root this file lives under.
    pub root: DeployRoot,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// FOMOD `<file priority=>`: when two selected options write the same
    /// destination, the higher priority wins regardless of option order.
    ///
    /// Zero everywhere else, which is exactly the behaviour the planner had
    /// before this existed — with every priority equal, the later option still
    /// wins. Nothing about the convention-driven formats changes.
    #[serde(default)]
    pub priority: i32,
}

/// One selectable option within a group (a single Fluffy option folder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModOption {
    /// Stable id within the bundle (derived from the folder name).
    pub id: String,
    /// The option's source folder name inside the archive.
    pub folder_name: String,
    /// The `-N-` group index this option belongs to, if any.
    pub group_index: Option<u32>,
    /// The slot token (`01`, `02`, `addon-01`, `segment`, `cover`, `warning`, ...).
    pub slot_token: Option<String>,
    /// For [`SelectMode::Exclusive`] options only: the key of the mutually-exclusive
    /// radio set this option belongs to. Options in one group can form several
    /// radio sets (e.g. `Physics-body-*` and `Physics-leg-*` are independent
    /// "pick one" choices). `None` for non-exclusive options.
    #[serde(default)]
    pub radio_set: Option<String>,
    /// Display name from `modinfo.ini` (`name=`), falling back to the folder name.
    pub name: String,
    /// Description from `modinfo.ini`, with literal `\n` already unescaped.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// `screenshot=` value from `modinfo.ini` (a filename inside the option folder).
    #[serde(default)]
    pub screenshot: Option<String>,
    /// Full in-archive path of the screenshot, resolved against the archive's
    /// real entries. `None` when `modinfo.ini` names an image that isn't present.
    #[serde(default)]
    pub screenshot_archive_path: Option<String>,
    pub select_mode: SelectMode,
    /// Pre-checked, but freely unchecked: FOMOD's `Recommended` plugin type.
    /// Distinct from [`SelectMode::Forced`], which the user cannot decline.
    #[serde(default)]
    pub recommended: bool,
    /// Why this option cannot be chosen, in the author's terms. Set for FOMOD's
    /// `NotUsable`, and rendered as a disabled entry carrying its reason rather
    /// than hidden — an option that silently vanishes reads as a bug in the
    /// manager, not as a decision by the mod author.
    #[serde(default)]
    pub blocked_reason: Option<String>,
    /// True iff this option carries a deployable payload (`natives/`/`reframework/`).
    pub deployable: bool,
    /// The files this option would deploy when selected.
    #[serde(default)]
    pub payload: Vec<FilePayload>,
    /// Raw `modinfo.ini` key/values, retained for round-trip and future fields.
    #[serde(default)]
    pub raw_modinfo: BTreeMap<String, String>,
}

impl ModOption {
    pub fn total_size(&self) -> u64 {
        self.payload.iter().map(|f| f.size).sum()
    }
}

/// A group of options sharing a `-N-` token (e.g. "-1- Helm"). The wizard renders
/// exclusive options as one radio set, stackable ones as checkboxes, etc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionGroup {
    /// The `-N-` index; `None` for options with no group token.
    pub index: Option<u32>,
    /// Human label derived from the folder naming (e.g. "Helm", "Body").
    pub label: String,
    /// How many of this group's options may or must be chosen, when the author
    /// said so. `None` for every format whose cardinality is inferred from
    /// folder names, which is the behaviour this field was added beside rather
    /// than replaced: a Fluffy radio set still means "at most one of these".
    #[serde(default)]
    pub cardinality: Option<GroupKind>,
    pub options: Vec<ModOption>,
}

impl OptionGroup {
    /// True if this group contains at least one mutually-exclusive (radio) option.
    pub fn has_exclusive(&self) -> bool {
        self.options
            .iter()
            .any(|o| o.select_mode == SelectMode::Exclusive)
    }

    /// The distinct radio-set keys present in this group, in first-seen order.
    /// Each key denotes one independent "pick at most one" choice.
    pub fn radio_sets(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for o in &self.options {
            if let Some(key) = &o.radio_set {
                if !seen.contains(key) {
                    seen.push(key.clone());
                }
            }
        }
        seen
    }
}

/// A fully normalized mod, ready to drive the install wizard and, once options
/// are chosen, a deployment plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModBundle {
    /// Logical bundle name (Fluffy `NameAsBundle`, else common prefix, else stem).
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    pub installer_model: InstallerModel,
    /// SHA-256 of the source archive, if computed.
    #[serde(default)]
    pub archive_sha256: Option<String>,
    /// The installer's own declaration of itself, for the one format that makes
    /// one. Carried alongside the flat groups rather than replacing them: the
    /// groups are what the wizard, the planner and the staged selection speak,
    /// and this is the program that decides which of them apply.
    ///
    /// `None` for every other format, and for every bundle stored before this
    /// existed.
    #[serde(default)]
    pub fomod: Option<FomodModule>,
    /// Files sitting at the archive root that no rule claimed, and that do not
    /// look like documentation.
    ///
    /// Recorded so the interface can say what it left behind. A mod whose root
    /// files the profile's patterns do not cover installs its payload and drops
    /// these, which is exactly the shape of the bug that made root files
    /// deployable in the first place — the difference is that now it says so.
    ///
    /// Empty for every bundle stored before this existed, and for the ordinary
    /// case where the only thing beside the payload is a readme.
    #[serde(default)]
    pub unclaimed_root_files: Vec<String>,
    pub groups: Vec<OptionGroup>,
}

impl ModBundle {
    /// Every option across all groups, in order.
    pub fn options(&self) -> impl Iterator<Item = &ModOption> {
        self.groups.iter().flat_map(|g| g.options.iter())
    }

    pub fn option_count(&self) -> usize {
        self.groups.iter().map(|g| g.options.len()).sum()
    }

    /// Options that carry a deployable payload.
    pub fn deployable_options(&self) -> impl Iterator<Item = &ModOption> {
        self.options().filter(|o| o.deployable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle serialised before FOMOD existed, exactly as `bundle_json` rows
    /// in an installed library hold it. Every field added since must default,
    /// or upgrading the app would fail to read a user's own mods back.
    const LEGACY_BUNDLE: &str = r#"{
        "name": "Some Mod",
        "version": "1.2",
        "author": null,
        "category": null,
        "installer_model": "fluffy-single",
        "archive_sha256": "abc",
        "groups": [{
            "index": null,
            "label": "Main",
            "options": [{
                "id": "main",
                "folder_name": "main",
                "group_index": null,
                "slot_token": null,
                "radio_set": null,
                "name": "Main",
                "description": null,
                "category": null,
                "author": null,
                "screenshot": null,
                "screenshot_archive_path": null,
                "select_mode": "forced",
                "deployable": true,
                "payload": [{
                    "archive_path": "main/natives/x.mesh",
                    "game_rel_path": "natives/x.mesh",
                    "root": "natives",
                    "size": 12
                }],
                "raw_modinfo": {}
            }]
        }]
    }"#;

    #[test]
    fn a_bundle_stored_before_fomod_existed_still_loads() {
        let bundle: ModBundle = serde_json::from_str(LEGACY_BUNDLE).expect("legacy bundle loads");

        assert_eq!(bundle.name, "Some Mod");
        assert!(bundle.fomod.is_none());
        assert_eq!(bundle.groups[0].cardinality, None);

        let option = &bundle.groups[0].options[0];
        assert!(!option.recommended);
        assert_eq!(option.blocked_reason, None);

        // Priority zero is not merely a placeholder: it is the value at which
        // the planner behaves exactly as it did before priorities existed.
        assert_eq!(option.payload[0].priority, 0);
    }

    #[test]
    fn a_bundle_round_trips_through_json_with_a_fomod_module_attached() {
        use crate::fomod::{FomodModule, SortOrder};

        let bundle = ModBundle {
            name: "Conditional Mod".into(),
            version: None,
            author: None,
            category: None,
            installer_model: InstallerModel::Fomod,
            archive_sha256: None,
            fomod: Some(FomodModule {
                name: "Conditional Mod".into(),
                image: None,
                module_dependencies: None,
                required_install_files: Vec::new(),
                install_steps: Vec::new(),
                step_order: SortOrder::Explicit,
                conditional_installs: Vec::new(),
                warnings: vec!["one construct was degraded".into()],
            }),
            unclaimed_root_files: Vec::new(),
            groups: Vec::new(),
        };

        let json = serde_json::to_string(&bundle).expect("serialises");
        let back: ModBundle = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, bundle);
    }

    #[test]
    fn the_fomod_installer_model_names_itself_in_json() {
        let json = serde_json::to_string(&InstallerModel::Fomod).expect("serialises");
        assert_eq!(json, "\"fomod\"");
    }
}
