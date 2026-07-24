//! Selection and deployment planning types.
//!
//! The flow is: a user's [`Selection`] over a [`crate::ModBundle`] resolves into a
//! [`DeploymentPlan`]: an explicit, inspectable list of file operations. Nothing
//! touches the game directory until a plan is applied, and every applied plan is
//! journaled so it can be reversed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How a file is physically placed into the game directory.
///
/// The ladder is chosen per source→target pair: reflink (CoW, instant, zero disk)
/// where the filesystem supports it, then hardlink on the same mount, then
/// symlink if the user opts into cross-mount space saving, else a real copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeployMethod {
    Reflink,
    Hardlink,
    Symlink,
    Copy,
}

impl DeployMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeployMethod::Reflink => "reflink",
            DeployMethod::Hardlink => "hardlink",
            DeployMethod::Symlink => "symlink",
            DeployMethod::Copy => "copy",
        }
    }
}

/// The set of options a user has chosen in the install wizard.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// Chosen [`crate::ModOption::id`] values.
    pub chosen: BTreeSet<String>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, option_id: &str) -> bool {
        self.chosen.contains(option_id)
    }

    pub fn insert(&mut self, option_id: impl Into<String>) {
        self.chosen.insert(option_id.into());
    }

    pub fn remove(&mut self, option_id: &str) {
        self.chosen.remove(option_id);
    }

    pub fn len(&self) -> usize {
        self.chosen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chosen.is_empty()
    }
}

/// A single file the plan will place into the game directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedFile {
    /// Id of the option that provides this file.
    pub option_id: String,
    /// Path inside the staged mod payload, relative to the mod's staging root.
    pub staged_rel_path: String,
    /// Destination path relative to the game install root.
    pub game_rel_path: String,
    pub size: u64,
}

/// Two or more selected options writing the same destination path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub game_rel_path: String,
    /// Option ids contending for this path, in resolution order (last wins).
    pub contenders: Vec<String>,
    /// The option id whose file is actually deployed.
    pub winner: String,
}

/// Why a selection is not valid to install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub message: String,
    /// Option ids the issue concerns.
    pub option_ids: Vec<String>,
}

/// A fully resolved, inspectable set of file operations. Produced from a bundle
/// plus a selection; consumed by the deploy engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub bundle_name: String,
    /// Files to place, already conflict-resolved (one entry per destination).
    pub files: Vec<PlannedFile>,
    /// Overlaps detected while resolving; informational, already applied above.
    pub conflicts: Vec<Conflict>,
    /// Reasons the selection is incomplete or contradictory.
    pub issues: Vec<ValidationIssue>,
}

impl DeploymentPlan {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// A plan is installable when it has files and no blocking issues.
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty() && !self.files.is_empty()
    }
}
