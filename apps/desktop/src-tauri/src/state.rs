//! Shared application state and the DTOs the UI consumes.
//!
//! The UI never sees domain internals directly; these serde types are the IPC
//! contract. Keeping them here means the React layer can be swapped without
//! touching the engines.

use apoc_domain::{ModBundle, Selection};
use apoc_storage::{Paths, Store};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

    /// Archives already analysed, keyed by path, most recent last.
    ///
    /// A memo and not a session. A conditional installer is re-evaluated on
    /// every click, and re-reading a multi-gigabyte archive each time to answer
    /// the same question would make the wizard unusable. Nothing here is state
    /// the user owns: losing it costs one re-read and never a wrong answer,
    /// which is why the answers themselves stay in the interface and arrive
    /// with each call.
    pub analyzed: Mutex<Vec<(String, std::sync::Arc<ModBundle>)>>,
}

/// Archives kept in [`AppState::analyzed`]. The wizard is modal, so one is
/// almost always enough; a few more cost little and cover going back and forth
/// between a download and the mod it replaces.
const ANALYZED_CACHE: usize = 8;

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
            analyzed: Mutex::new(Vec::new()),
        })
    }

    /// A previously analysed archive, if it is still remembered.
    pub fn cached_analysis(&self, archive_path: &str) -> Option<std::sync::Arc<ModBundle>> {
        let mut cache = self.analyzed.lock().ok()?;
        let at = cache.iter().position(|(p, _)| p == archive_path)?;
        // Move it to the end so the least recently wanted falls off first.
        let entry = cache.remove(at);
        let bundle = entry.1.clone();
        cache.push(entry);
        Some(bundle)
    }

    /// Remember an analysed archive.
    pub fn remember_analysis(&self, archive_path: &str, bundle: &ModBundle) {
        let Ok(mut cache) = self.analyzed.lock() else {
            return;
        };
        cache.retain(|(p, _)| p != archive_path);
        cache.push((
            archive_path.to_string(),
            std::sync::Arc::new(bundle.clone()),
        ));
        while cache.len() > ANALYZED_CACHE {
            cache.remove(0);
        }
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
    /// The installer's own suggestion, pre-ticked and freely changed.
    #[serde(default)]
    pub recommended: bool,
    /// Why this option cannot be chosen, when it cannot. Shown rather than
    /// hidden, so a choice the author disabled does not read as a missing one.
    #[serde(default)]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub index: Option<u32>,
    pub label: String,
    /// Distinct radio-set keys in this group (each is one independent choice).
    pub radio_sets: Vec<String>,
    /// How many options may or must be chosen, when the installer said so:
    /// `select-exactly-one` | `select-at-most-one` | `select-at-least-one` |
    /// `select-all` | `select-any`. Absent where cardinality is inferred.
    #[serde(default)]
    pub cardinality: Option<String>,
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
    /// What an installer asked for that could not be honoured exactly, in the
    /// author's own terms. Shown rather than swallowed: a silently degraded
    /// install looks identical to a correct one until the game misbehaves.
    #[serde(default)]
    pub warnings: Vec<String>,
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

/// An installed mod an archive appears to be a new version of.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceCandidateView {
    pub mod_id: String,
    pub name: String,
    pub version: Option<String>,
    /// True when the match identifies the mod rather than guessing at it. A
    /// certain match replaces the library row without asking; an uncertain one
    /// is a question for the person installing, because the only evidence is a
    /// shared name and names are neither stable nor unique.
    pub certain: bool,
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
    /// The mod this archive appears to update, when it appears to update one.
    pub replaces: Option<ReplaceCandidateView>,
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
    /// What writing the game's plugin list changed, for a game that has one.
    ///
    /// `None` for every game ordered by mod priority, which is all of them but
    /// the Creation Engine ones.
    pub plugin_list: Option<PluginListResultView>,
}

/// The plugin-list half of a deployment's outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginListResultView {
    /// Plugins that were not in the list before and now are.
    pub added: Vec<String>,
    /// Plugins that load before something they depend on, each as a sentence.
    ///
    /// Reported rather than repaired: the order is the user's, and this says
    /// what is wrong so they can decide, which is the whole difference between
    /// a manager and a tool that rearranges your game behind you.
    pub problems: Vec<String>,
    /// Whether the list file was actually written.
    pub written: bool,
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
            cardinality: g.cardinality.map(|k| {
                match k {
                    apoc_domain::fomod::GroupKind::SelectExactlyOne => "select-exactly-one",
                    apoc_domain::fomod::GroupKind::SelectAtMostOne => "select-at-most-one",
                    apoc_domain::fomod::GroupKind::SelectAtLeastOne => "select-at-least-one",
                    apoc_domain::fomod::GroupKind::SelectAll => "select-all",
                    apoc_domain::fomod::GroupKind::SelectAny => "select-any",
                }
                .to_string()
            }),
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
                    recommended: o.recommended,
                    blocked_reason: o.blocked_reason.clone(),
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

/// Resolve the staging directory holding one archive's extracted files.
///
/// Takes the mod's `staging_key`, not its id. Reading it off the record is what
/// keeps a mod that has been updated pointed at its current generation while the
/// previous one stays where an applied deployment expects it.
pub fn staging_for(paths: &Paths, game_id: &str, staging_key: &str) -> PathBuf {
    paths.staging_dir(game_id, staging_key)
}

/// Delete staging directories no mod claims any more, returning how many went.
///
/// Driven entirely by `keep`: a directory is removed because nothing in the
/// library names it, never because its name looks stale. That is the whole
/// safety argument, and it is why this takes a keep-list rather than working out
/// for itself what looks abandoned — a bug in a "looks abandoned" rule deletes
/// files a deployment needs, and a bug here leaves a directory behind.
///
/// Loose files directly under the root are left alone. Nothing puts them there,
/// so one is a sign of something this function does not understand, and the
/// conservative answer is to not touch it.
pub fn prune_staging(staging_root: &Path, keep: &HashSet<String>) -> std::io::Result<usize> {
    let Ok(entries) = std::fs::read_dir(staging_root) else {
        // No staging root yet: nothing has ever been imported for this game.
        return Ok(0);
    };

    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if keep.contains(&name) {
            continue;
        }
        std::fs::remove_dir_all(entry.path())?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod prune_staging_tests {
    use super::*;

    fn dir(root: &Path, name: &str) -> PathBuf {
        let p = root.join(name);
        std::fs::create_dir_all(p.join("opt")).unwrap();
        std::fs::write(p.join("opt/a.pak"), b"bytes").unwrap();
        p
    }

    fn keep(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn removes_only_what_the_library_no_longer_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let live = dir(root, "mod-a__v2");
        let stale = dir(root, "mod-a__v1");
        let other = dir(root, "mod-b__v1");

        let removed = prune_staging(root, &keep(&["mod-a__v2", "mod-b__v1"])).unwrap();

        assert_eq!(removed, 1);
        assert!(!stale.exists(), "the superseded generation is reclaimed");
        assert!(live.exists(), "the current one is not");
        assert!(other.exists(), "nor is an unrelated mod's");
    }

    #[test]
    fn an_empty_keep_list_is_taken_at_its_word() {
        // A game with no mods left really does have no staging worth keeping.
        // Treating "keep nothing" as "something must be wrong, keep everything"
        // would mean the directory could never be reclaimed.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        dir(root, "mod-a__v1");
        dir(root, "mod-b__v1");

        assert_eq!(prune_staging(root, &keep(&[])).unwrap(), 2);
    }

    #[test]
    fn loose_files_are_left_alone() {
        // Nothing puts a file directly under the staging root, so one is a sign
        // of something this function does not understand. Deleting what you do
        // not recognise is how a prune becomes a bug report.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let stray = root.join("notes.txt");
        std::fs::write(&stray, b"why is this here").unwrap();
        dir(root, "mod-a__v1");

        assert_eq!(prune_staging(root, &keep(&[])).unwrap(), 1);
        assert!(stray.exists(), "only directories are pruned");
    }

    #[test]
    fn a_game_that_has_never_staged_anything_is_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("games/never-played/staging");
        assert_eq!(prune_staging(&missing, &keep(&["mod-a"])).unwrap(), 0);
    }
}
