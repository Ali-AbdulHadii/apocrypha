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
    /// In-flight and finished downloads. Not persisted: a transfer cannot
    /// survive a restart anyway, and finished files are found by scanning.
    pub downloads: std::sync::Arc<crate::downloads::Queue>,
    /// The sign-in waiting for a browser to answer, if one is.
    ///
    /// Held here rather than handed to the interface because it owns two
    /// secrets — the PKCE verifier and the callback's `state` — and a listening
    /// socket. `Some` is also what "a sign-in is in flight" means, so starting
    /// a second one closes the first rather than leaving an orphaned port open.
    pub pending_authorization: Mutex<Option<apoc_apocrypha::PendingAuthorization>>,

    /// Cancel flag for the deployment currently running, if one is.
    ///
    /// `Some` is also what "a deploy is in flight" means, so a second Apply can
    /// be refused rather than allowed to interleave writes with the first.
    pub deploy_cancel: Mutex<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>>,
}

/// Setting holding a user-chosen downloads folder. Unset means the default.
pub const KEY_DOWNLOADS_DIR: &str = "downloads_dir";

impl AppState {
    pub fn new() -> Result<Self, String> {
        let paths = Paths::default();
        let store = Store::open(&paths.database()).map_err(|e| e.to_string())?;
        Ok(AppState {
            store: Mutex::new(store),
            paths,
            downloads: Default::default(),
            pending_authorization: Mutex::new(None),
            deploy_cancel: Mutex::new(None),
        })
    }

    /// Where downloads are kept, inside the app's data directory by default.
    pub fn default_downloads_dir(&self) -> PathBuf {
        self.paths.root().join("downloads")
    }

    /// Where downloads are kept right now.
    ///
    /// Configurable because a mod library is measured in tens of gigabytes and
    /// often belongs on a different disk than the application data, and because
    /// pointing it at an existing folder is the quickest way to bring downloads
    /// from another manager across.
    pub fn downloads_dir(&self) -> PathBuf {
        self.store
            .lock()
            .ok()
            .and_then(|s| s.get_setting(KEY_DOWNLOADS_DIR).ok().flatten())
            .filter(|p| !p.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_downloads_dir())
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
    /// Nexus Mods domain, so an incoming `nxm://` link can be routed to the
    /// game it is actually for rather than to whichever game is on screen.
    pub nexus_domain: Option<String>,
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

/// What happened when a previous install's choices were moved onto a new
/// version of the same mod.
///
/// Names rather than ids, because this is shown to a person. A dropped option
/// may not exist in the new bundle at all, so there is no name to resolve and
/// its id — the folder name it had — is the best that can be said about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarryView {
    /// Options kept from the last install, in bundle order.
    pub carried: Vec<String>,
    /// Chosen before, absent or no longer selectable now.
    pub dropped: Vec<String>,
    /// Choice sets in the new version with nothing chosen, as raw radio-set
    /// keys. The UI prettifies them with the same rule the wizard already uses
    /// for set headings, so the two cannot disagree about what a set is called.
    pub undecided: Vec<String>,
    /// True when nothing needs asking and the wizard can be skipped.
    pub complete: bool,
}

/// An analyzed archive, plus the carry outcome when it is a new version of
/// something already installed.
///
/// Separate from [`ModView`] rather than a field on it: carrying only ever
/// applies to this one command, and `list_mods` returning a permanently null
/// column would invite the UI to look for it where it can never be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedArchive {
    #[serde(rename = "mod")]
    pub mod_view: ModView,
    /// `None` when nothing by this name is installed, so there was nothing to
    /// carry and every choice is being made for the first time.
    pub carry: Option<CarryView>,
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
    pub downloads_dir: String,
    /// False once the user has chosen their own folder, so the interface can
    /// offer to put it back without having to know what the default is.
    pub downloads_dir_is_default: bool,
}

/// How far the running deployment has got. Emitted as `deploy-progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyProgressView {
    /// `reverting` while the previous deployment is undone, then `linking`.
    pub phase: String,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
}

/// How the deployment ended. Emitted once, as `deploy-finished`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployOutcomeView {
    pub cancelled: bool,
    pub result: Option<DeployResultView>,
    pub error: Option<String>,
    /// Set when a cancel could not put everything back, so the interface can say
    /// so instead of reporting a clean stop.
    pub rollback: Option<RollbackView>,
}

/// One deployed file whose on-disk state is not what the journal describes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVerdictView {
    pub path: String,
    /// `missing` or `modified`.
    pub state: String,
    pub repairable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReportView {
    pub checked: usize,
    pub ok: usize,
    pub problems: Vec<FileVerdictView>,
    pub intact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairReportView {
    pub repaired: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// One folder Apocrypha owns, and what it currently costs on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntryView {
    pub label: String,
    pub path: String,
    pub bytes: u64,
    /// One line saying what lives there, in the user's terms.
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsageView {
    pub entries: Vec<UsageEntryView>,
    pub total: u64,
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
