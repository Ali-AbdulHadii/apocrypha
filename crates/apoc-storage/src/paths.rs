//! The on-disk layout, in the place each platform keeps such things.
//!
//! Everything Apocrypha owns lives under one root: `$XDG_DATA_HOME/apocrypha`
//! (default `~/.local/share/apocrypha`) on Unix, `%LOCALAPPDATA%\Apocrypha` on
//! Windows. Staged mods, the vault, and journals are kept **outside the game
//! directory** so the game install stays disposable.
//!
//! Only the root differs. Everything below it is the same shape everywhere,
//! which is what keeps a journal written on one machine legible on another and
//! keeps every path rule in one function rather than two divergent copies.
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
    // USERPROFILE on Windows, HOME elsewhere. Falling back to the working
    // directory rather than panicking is deliberate: a missing home is a broken
    // environment, and the app is more useful complaining about a path it cannot
    // write than refusing to start.
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where this platform keeps per-user application data.
///
/// The two conventions differ in more than spelling. XDG says lowercase
/// directories under a dotted path; Windows expects a capitalised vendor or
/// application folder directly under `Local`. Following each rather than
/// imposing one means the directory looks native wherever someone finds it.
#[cfg(unix)]
fn default_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".local/share"))
        .join("apocrypha")
}

#[cfg(windows)]
fn default_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join("AppData").join("Local"))
        .join("Apocrypha")
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            root: default_root(),
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

    /// Staging directory for one archive's extracted files.
    ///
    /// Keyed by staging key rather than mod id: a mod keeps its id across an
    /// update, but each archive gets its own directory, so replacing a version
    /// never writes over bytes an applied deployment still repairs from.
    pub fn staging_dir(&self, game_id: &str, staging_key: &str) -> PathBuf {
        self.staging_root(game_id).join(sanitize(staging_key))
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

    // A trailing dot is legal here and rejected by Windows, which strips it
    // silently in some APIs and errors in others — so "wilds." and "wilds"
    // become the same directory on one platform and a failure on the other.
    let trimmed = mapped.trim_end_matches('.');
    if trimmed.is_empty() {
        return "_".to_string();
    }

    // Windows reserves a handful of device names, and reserves them *with any
    // extension*: CON, NUL and COM1 are unusable as file or directory names, so
    // a game whose id happened to be one of them would be undeployable there and
    // fine here. Suffixed rather than replaced, so the name stays recognisable.
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    if is_reserved_device_name(stem) {
        return format!("{trimmed}_");
    }

    trimmed.to_string()
}

/// The MS-DOS device names Windows still refuses, case-insensitively.
///
/// Applied on every platform rather than behind `cfg(windows)`. These paths end
/// up in a journal, and a journal written on Linux should describe a layout that
/// a Windows build could also produce — a name that only works on one of them is
/// a portability bug that surfaces years later, when someone moves a library.
fn is_reserved_device_name(stem: &str) -> bool {
    const RESERVED: [&str; 4] = ["con", "prn", "aux", "nul"];
    let lower = stem.to_ascii_lowercase();
    if RESERVED.contains(&lower.as_str()) {
        return true;
    }
    // COM1-COM9 and LPT1-LPT9.
    matches!(lower.strip_prefix("com").or_else(|| lower.strip_prefix("lpt")),
        Some(rest) if rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_windows_reserves_is_made_usable() {
        // These are unusable as file or directory names on Windows, extension or
        // not. A game id that happened to be one would deploy here and fail
        // there, which is the kind of difference that shows up long after
        // anybody remembers this code.
        assert_eq!(sanitize("con"), "con_");
        assert_eq!(sanitize("CON"), "CON_");
        assert_eq!(sanitize("nul.txt"), "nul.txt_");
        assert_eq!(sanitize("com1"), "com1_");
        assert_eq!(sanitize("lpt9"), "lpt9_");

        // And names that merely start the same way are left alone.
        assert_eq!(sanitize("console"), "console");
        assert_eq!(sanitize("com10"), "com10");
        assert_eq!(sanitize("connie"), "connie");
    }

    #[test]
    fn a_trailing_dot_is_removed_rather_than_carried() {
        // Windows strips it silently in some APIs and errors in others, so
        // "wilds." and "wilds" would be one directory on one platform and a
        // failure on the other.
        assert_eq!(sanitize("wilds."), "wilds");
        assert_eq!(sanitize("wilds..."), "wilds");
        assert_eq!(sanitize("..."), "_");
    }

    #[test]
    fn layout_is_nested_under_the_game_id() {
        let p = Paths::with_root("/data/apocrypha");
        assert_eq!(
            p.staging_dir("monster-hunter-wilds", "mod-1"),
            PathBuf::from("/data/apocrypha/games/monster-hunter-wilds/staging/mod-1")
        );
        assert!(p.vault("g").ends_with("games/g/vault"));
        assert!(p.journal("g").ends_with("games/g/journal"));
    }

    #[test]
    fn ids_are_sanitized_into_one_segment() {
        let p = Paths::with_root("/data");
        let staged = p.staging_dir("../etc", "../../passwd");
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
