//! Shared application state and the DTOs the UI consumes.
//!
//! The UI never sees domain internals directly; these serde types are the IPC
//! contract. Keeping them here means the React layer can be swapped without
//! touching the engines.

use apoc_domain::{ModBundle, Selection};
use apoc_storage::{Paths, Store};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub store: Mutex<Store>,
    pub paths: Paths,
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let paths = Paths::default();
        let store = Store::open(&paths.database()).map_err(|e| e.to_string())?;
        Ok(AppState {
            store: Mutex::new(store),
            paths,
        })
    }
}

/// A game as the UI shows it: definition + detection results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub steam_app_id: u32,
    pub load_order: String,
    /// Detected or user-set install directory.
    pub install_dir: Option<String>,
    pub proton_prefix: Option<String>,
    pub proton_tool: Option<String>,
    pub detected: bool,
    /// Loader name, e.g. "REFramework".
    pub loader_name: Option<String>,
    pub loader_dll: Option<String>,
    /// Whether the DLL override is currently registered in the prefix.
    pub loader_override_active: bool,
    pub steam_launch_options: Option<String>,
}

/// One wizard option, flattened for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// `forced` | `exclusive` | `stackable` | `info`
    pub select_mode: String,
    pub radio_set: Option<String>,
    pub deployable: bool,
    pub file_count: usize,
    pub size_bytes: u64,
    pub screenshot: Option<String>,
    /// True when a preview image is available for this option.
    pub has_preview: bool,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub index: Option<u32>,
    pub label: String,
    /// Distinct radio-set keys in this group (each is one independent choice).
    pub radio_sets: Vec<String>,
    pub options: Vec<OptionView>,
}

/// A mod as the UI shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModView {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub installer_model: String,
    pub enabled: bool,
    pub priority: i64,
    /// True when this mod's files are currently in the game folder.
    pub applied: bool,
    /// Unix seconds when the mod was imported, for "recently added" sorting.
    pub added_at: i64,
    pub groups: Vec<GroupView>,
    /// Currently chosen option ids.
    pub selection: Vec<String>,
    pub total_files: usize,
    pub total_bytes: u64,
}

/// Result of previewing a deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunView {
    pub method: String,
    pub creates: Vec<String>,
    pub replaces: Vec<String>,
    pub missing: Vec<String>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub conflicts: Vec<ConflictView>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    pub path: String,
    pub contenders: Vec<String>,
    pub winner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployResultView {
    pub deployment_id: String,
    pub files_deployed: usize,
    pub bytes: u64,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackView {
    pub removed: usize,
    pub restored: usize,
    pub skipped_modified: Vec<String>,
    pub errors: Vec<String>,
    pub clean: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: i64,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub game_db_source: String,
    pub data_root: String,
    pub deploy_method_preference: String,
}

/// Build the UI view of a bundle's groups.
pub fn groups_view(bundle: &ModBundle) -> Vec<GroupView> {
    bundle
        .groups
        .iter()
        .map(|g| GroupView {
            index: g.index,
            label: g.label.clone(),
            radio_sets: g.radio_sets(),
            options: g
                .options
                .iter()
                .map(|o| OptionView {
                    id: o.id.clone(),
                    name: o.name.clone(),
                    description: o.description.clone(),
                    select_mode: match o.select_mode {
                        apoc_domain::SelectMode::Forced => "forced",
                        apoc_domain::SelectMode::Exclusive => "exclusive",
                        apoc_domain::SelectMode::Stackable => "stackable",
                        apoc_domain::SelectMode::Info => "info",
                    }
                    .to_string(),
                    radio_set: o.radio_set.clone(),
                    deployable: o.deployable,
                    file_count: o.payload.len(),
                    size_bytes: o.total_size(),
                    screenshot: o.screenshot.clone(),
                    has_preview: o.screenshot_archive_path.is_some(),
                    category: o.category.clone(),
                })
                .collect(),
        })
        .collect()
}

pub fn selection_vec(sel: &Selection) -> Vec<String> {
    sel.chosen.iter().cloned().collect()
}

pub fn selection_from(ids: &[String]) -> Selection {
    let mut s = Selection::new();
    for id in ids {
        s.insert(id.clone());
    }
    s
}

/// Resolve the staging directory for a mod.
pub fn staging_for(paths: &Paths, game_id: &str, mod_id: &str) -> PathBuf {
    paths.mod_staging(game_id, mod_id)
}
