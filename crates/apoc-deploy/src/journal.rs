//! The append-only deployment journal.
//!
//! Every mutation of the game directory is recorded *before* it is considered
//! done, as one JSON object per line. Rollback replays the journal in reverse.
//! Because the journal is append-only and each op is self-describing, an
//! interrupted deploy is still fully reversible.

use apoc_domain::DeployMethod;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// One reversible operation performed against the game directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum JournalOp {
    /// A file that did not exist before was created by us.
    Created {
        game_rel_path: String,
        /// Staged source this was placed from, relative to the staging root.
        ///
        /// Recorded so a later verify/repair pass can re-place a file that went
        /// missing: knowing only the destination, the bytes are unrecoverable
        /// once the link is broken. Defaulted because journals written before
        /// this field existed must still load, and an empty string reads as
        /// "source unknown, repair not possible".
        #[serde(default)]
        staged_rel_path: String,
        sha256: String,
        method: DeployMethod,
    },
    /// A pre-existing file was replaced; the original is preserved in the vault.
    Replaced {
        game_rel_path: String,
        /// Staged source this was placed from, relative to the staging root.
        /// See [`JournalOp::Created`] for why it is recorded and defaulted.
        #[serde(default)]
        staged_rel_path: String,
        /// Vault key (content hash) of the ORIGINAL file.
        original_vault_key: String,
        /// Hash of the file we wrote.
        sha256: String,
        method: DeployMethod,
    },
    /// The loader DLL was placed in the game root (always a real copy).
    LoaderInstalled {
        game_rel_path: String,
        /// Vault key of the original DLL, when one already existed.
        original_vault_key: Option<String>,
    },
    /// A game's plugin list was written, in the Proton prefix.
    ///
    /// Recorded with an absolute path rather than a game-relative one, because
    /// this file is not in the game directory and nothing relative to it would
    /// name the same place twice.
    PluginListWritten {
        /// Absolute path to the file written.
        path: String,
        /// Vault key of the list that was there before, when there was one.
        ///
        /// `None` means the game had never written a list, so rolling back
        /// means removing ours rather than restoring anything.
        original_vault_key: Option<String>,
        /// Hash of what we wrote, so rollback can tell our list from the one
        /// the user curated afterwards.
        sha256: String,
    },
    /// A Wine DLL override was written into the Proton prefix registry.
    RegistryOverride {
        /// The registry value name, e.g. `dinput8`.
        name: String,
        /// The value we wrote, e.g. `native,builtin`.
        value: String,
        /// The value that was there before, if any (restored on rollback).
        previous: Option<String>,
    },
}

/// Journal header, written as the first line of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentHeader {
    pub id: String,
    pub game_id: String,
    pub bundle_name: String,
    /// Seconds since the Unix epoch.
    pub created_at: u64,
    pub game_dir: String,
}

/// A journal being written during a deploy, or read back for rollback.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    header: DeploymentHeader,
    ops: Vec<JournalOp>,
}

impl Journal {
    /// Create a new journal file and write its header.
    pub fn create(dir: &Path, header: DeploymentHeader) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.jsonl", header.id));
        let mut f = File::create(&path)?;
        writeln!(f, "{}", serde_json::to_string(&header)?)?;
        f.sync_all()?;
        Ok(Journal {
            path,
            header,
            ops: Vec::new(),
        })
    }

    /// Append an op and flush it to disk before the caller proceeds.
    pub fn append(&mut self, op: JournalOp) -> std::io::Result<()> {
        let mut f = OpenOptions::new().append(true).open(&self.path)?;
        writeln!(f, "{}", serde_json::to_string(&op)?)?;
        f.sync_all()?;
        self.ops.push(op);
        Ok(())
    }

    /// Load a journal from disk.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let f = File::open(path)?;
        let mut lines = BufReader::new(f).lines();
        let header_line = lines.next().transpose()?.unwrap_or_default();
        let header: DeploymentHeader = serde_json::from_str(&header_line)?;
        let mut ops = Vec::new();
        for line in lines {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // A torn final line from an interrupted write is skipped, not fatal.
            if let Ok(op) = serde_json::from_str::<JournalOp>(&line) {
                ops.push(op);
            }
        }
        Ok(Journal {
            path: path.to_path_buf(),
            header,
            ops,
        })
    }

    pub fn header(&self) -> &DeploymentHeader {
        &self.header
    }

    pub fn ops(&self) -> &[JournalOp] {
        &self.ops
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn id(&self) -> &str {
        &self.header.id
    }
}

/// Seconds since the Unix epoch (0 if the clock is before it).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A sortable, collision-resistant-enough deployment id: epoch nanos in hex.
pub fn new_deployment_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("dep-{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips_header_and_ops() {
        let dir = tempdir().unwrap();
        let header = DeploymentHeader {
            id: new_deployment_id(),
            game_id: "monster-hunter-wilds".into(),
            bundle_name: "Test".into(),
            created_at: now_secs(),
            game_dir: "/games/mhw".into(),
        };
        let mut j = Journal::create(dir.path(), header.clone()).unwrap();
        j.append(JournalOp::Created {
            game_rel_path: "natives/a.mesh.1".into(),
            staged_rel_path: "opt/natives/a.mesh.1".into(),
            sha256: "abc".into(),
            method: DeployMethod::Hardlink,
        })
        .unwrap();
        j.append(JournalOp::RegistryOverride {
            name: "dinput8".into(),
            value: "native,builtin".into(),
            previous: None,
        })
        .unwrap();

        let loaded = Journal::load(j.path()).unwrap();
        assert_eq!(loaded.header(), &header);
        assert_eq!(loaded.ops().len(), 2);
    }

    #[test]
    fn tolerates_a_torn_trailing_line() {
        let dir = tempdir().unwrap();
        let header = DeploymentHeader {
            id: "dep-x".into(),
            game_id: "g".into(),
            bundle_name: "b".into(),
            created_at: 0,
            game_dir: "/g".into(),
        };
        let mut j = Journal::create(dir.path(), header).unwrap();
        j.append(JournalOp::Created {
            game_rel_path: "a".into(),
            staged_rel_path: "opt/a".into(),
            sha256: "h".into(),
            method: DeployMethod::Copy,
        })
        .unwrap();
        // Simulate a crash mid-write.
        let mut f = OpenOptions::new().append(true).open(j.path()).unwrap();
        write!(f, "{{\"op\":\"crea").unwrap();

        let loaded = Journal::load(j.path()).unwrap();
        assert_eq!(
            loaded.ops().len(),
            1,
            "partial line ignored, valid ops kept"
        );
    }

    #[test]
    fn loads_a_journal_written_before_staged_paths_were_recorded() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dep-old.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"id":"dep-old","game_id":"g","bundle_name":"b","created_at":0,"game_dir":"/g"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"op":"created","game_rel_path":"natives/a.pak","sha256":"h","method":"hardlink"}}"#
        )
        .unwrap();

        let loaded = Journal::load(&path).unwrap();
        assert_eq!(
            loaded.ops(),
            &[JournalOp::Created {
                game_rel_path: "natives/a.pak".into(),
                staged_rel_path: String::new(),
                sha256: "h".into(),
                method: DeployMethod::Hardlink,
            }],
            "an older op still rolls back, it just cannot be repaired"
        );
    }
}
