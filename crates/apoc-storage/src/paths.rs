//! XDG-first on-disk layout.
//!
//! Everything Apocrypha owns lives under `$XDG_DATA_HOME/apocrypha` (default
//! `~/.local/share/apocrypha`). Staged mods, the vault, and journals are kept
//! **outside the game directory** so the game install stays disposable.
//!
//! ```text
//! ~/.local/share/apocrypha/
//! ├─ apocrypha.db
//! └─ games/<game-id>/
//!     ├─ staging/<mod-id>/      extracted payloads, namespaced per option
//!     ├─ vault/<aa>/<hash…>     original game files displaced by a deploy
//!     └─ journal/<dep-id>.jsonl append-only deployment log
//! ```

use std::path::{Path, PathBuf};

/// Resolved application directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Default for Paths {
    fn default() -> Self {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home().join(".local/share"));
        Paths {
            root: base.join("apocrypha"),
        }
    }
}

impl Paths {
    /// Use an explicit root (tests, portable installs).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Paths { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn database(&self) -> PathBuf {
        self.root.join("apocrypha.db")
    }

    pub fn game_dir(&self, game_id: &str) -> PathBuf {
        self.root.join("games").join(sanitize(game_id))
    }

    pub fn staging_root(&self, game_id: &str) -> PathBuf {
        self.game_dir(game_id).join("staging")
    }

    /// Staging directory for one installed mod.
    pub fn mod_staging(&self, game_id: &str, mod_id: &str) -> PathBuf {
        self.staging_root(game_id).join(sanitize(mod_id))
    }

    pub fn vault(&self, game_id: &str) -> PathBuf {
        self.game_dir(game_id).join("vault")
    }

    pub fn journal(&self, game_id: &str) -> PathBuf {
        self.game_dir(game_id).join("journal")
    }

    /// Create the directories needed for one game.
    pub fn ensure_game_dirs(&self, game_id: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(self.staging_root(game_id))?;
        std::fs::create_dir_all(self.vault(game_id))?;
        std::fs::create_dir_all(self.journal(game_id))?;
        Ok(())
    }
}

/// Reduce an id to exactly one safe path segment: no separators, and never a
/// relative-directory name (`.`, `..`) that could escape the parent directory.
fn sanitize(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '_',
        })
        .collect();
    if mapped.is_empty() || mapped.chars().all(|c| c == '.') {
        return "_".to_string();
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_nested_under_the_game_id() {
        let p = Paths::with_root("/data/apocrypha");
        assert_eq!(
            p.mod_staging("monster-hunter-wilds", "mod-1"),
            PathBuf::from("/data/apocrypha/games/monster-hunter-wilds/staging/mod-1")
        );
        assert!(p.vault("g").ends_with("games/g/vault"));
        assert!(p.journal("g").ends_with("games/g/journal"));
    }

    #[test]
    fn ids_are_sanitized_into_one_segment() {
        let p = Paths::with_root("/data");
        let staged = p.mod_staging("../etc", "../../passwd");
        assert!(staged.starts_with("/data/games"));
        // The real invariant: hostile ids contribute exactly one path segment
        // and can never walk upward.
        assert!(!staged
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)));
        assert_eq!(
            staged.components().count(),
            PathBuf::from("/data/games/x/staging/y")
                .components()
                .count()
        );
    }

    #[test]
    fn pure_dot_ids_never_become_relative_directories() {
        assert_eq!(sanitize(".."), "_");
        assert_eq!(sanitize("."), "_");
        assert_eq!(sanitize(""), "_");
        assert_eq!(sanitize("mod-1.03"), "mod-1.03");
    }
}
