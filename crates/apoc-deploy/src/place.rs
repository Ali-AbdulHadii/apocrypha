//! The adaptive-link placement ladder.
//!
//! For each staged→game file pair we pick the cheapest method that is correct on
//! this filesystem, in order: **reflink** (CoW, instant, zero extra disk) →
//! **hardlink** (same mount) → **symlink** (only when explicitly allowed) →
//! **copy** (always correct).
//!
//! RE Engine loose files are *additive*: they are not part of the Steam depot: //! so a link is indistinguishable from a copy to the game and to Steam's verify.
//! The loader DLL is the deliberate exception and is always a real copy.

use apoc_domain::DeployMethod;
use std::fs;
use std::io;
use std::path::Path;

/// Which methods a deployment is allowed to use, best-first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ladder {
    pub methods: Vec<DeployMethod>,
}

impl Default for Ladder {
    /// Reflink, then hardlink, then copy. Symlinks are opt-in because some tools
    /// (and Windows-side game code under Proton) treat them inconsistently.
    ///
    /// The same three rungs on every platform, and that is deliberate: reflink
    /// asks the filesystem and fails harmlessly where it is not supported
    /// (which on Windows is everything but ReFS), and hardlinks work on NTFS
    /// within a volume exactly as they do on ext4 within a mount. The ladder was
    /// always a question put to the filesystem rather than to the operating
    /// system, so it needs no per-platform variant — only `with_symlinks` does,
    /// and that one is already opt-in.
    fn default() -> Self {
        Ladder {
            methods: vec![
                DeployMethod::Reflink,
                DeployMethod::Hardlink,
                DeployMethod::Copy,
            ],
        }
    }
}

impl Ladder {
    /// A ladder that only ever makes real copies (maximum compatibility).
    pub fn copy_only() -> Self {
        Ladder {
            methods: vec![DeployMethod::Copy],
        }
    }

    /// Include symlinks (space-saving across mount boundaries).
    ///
    /// Worth knowing before offering this on Windows: creating a symlink there
    /// requires Developer Mode or elevation, so for most users the rung fails
    /// and the ladder falls through to a copy. That is safe but silent — the
    /// setting appears to do nothing rather than announcing why.
    pub fn with_symlinks() -> Self {
        Ladder {
            methods: vec![
                DeployMethod::Reflink,
                DeployMethod::Hardlink,
                DeployMethod::Symlink,
                DeployMethod::Copy,
            ],
        }
    }
}

fn try_method(method: DeployMethod, src: &Path, dest: &Path) -> io::Result<()> {
    match method {
        DeployMethod::Reflink => reflink_copy::reflink(src, dest),
        DeployMethod::Hardlink => fs::hard_link(src, dest),
        DeployMethod::Symlink => symlink_file(src, dest),
        DeployMethod::Copy => fs::copy(src, dest).map(|_| ()),
    }
}

/// A file symlink, spelled per platform.
///
/// The two are not equivalent in practice, which is why this is a function
/// rather than a `use`. On Unix any process may create one. On Windows it needs
/// either Developer Mode or elevation, so the call fails for ordinary users with
/// a permission error — which is correct behaviour for the ladder, since it
/// simply moves down to the next rung, but means a symlink there is a fallback
/// nobody should plan around.
#[cfg(unix)]
fn symlink_file(src: &Path, dest: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dest)
}

#[cfg(windows)]
fn symlink_file(src: &Path, dest: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(src, dest)
}

/// Place `src` at `dest`, trying each allowed method in order.
///
/// `dest` must not already exist (callers vault-and-remove first), so a failed
/// attempt can be retried with the next method without ambiguity.
pub fn place(ladder: &Ladder, src: &Path, dest: &Path) -> io::Result<DeployMethod> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut last_err: Option<io::Error> = None;
    for &method in &ladder.methods {
        match try_method(method, src, dest) {
            Ok(()) => return Ok(method),
            Err(e) => {
                // Clean up a partial artifact before trying the next rung.
                let _ = fs::remove_file(dest);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| io::Error::other("no deployment method available")))
}

/// Probe which method the ladder would actually use between two directories.
/// Used by the UI to show "hardlink (zero extra disk)" before the user commits.
pub fn probe(ladder: &Ladder, staging_dir: &Path, game_dir: &Path) -> io::Result<DeployMethod> {
    fs::create_dir_all(staging_dir)?;
    fs::create_dir_all(game_dir)?;
    let src = staging_dir.join(".apoc-probe-src");
    let dest = game_dir.join(".apoc-probe-dest");
    let _ = fs::remove_file(&dest);
    fs::write(&src, b"apocrypha probe")?;
    let result = place(ladder, &src, &dest);
    let _ = fs::remove_file(&src);
    let _ = fs::remove_file(&dest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn places_a_file_and_reports_the_method_used() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("nested/dest.bin");
        fs::write(&src, b"payload").unwrap();

        let method = place(&Ladder::default(), &src, &dest).unwrap();
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"payload");
        // On ext4 reflink is unavailable, so the ladder falls to hardlink.
        assert!(matches!(
            method,
            DeployMethod::Reflink | DeployMethod::Hardlink | DeployMethod::Copy
        ));
    }

    #[test]
    fn copy_only_ladder_produces_an_independent_file() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.bin");
        let dest = dir.path().join("dest.bin");
        fs::write(&src, b"one").unwrap();

        let method = place(&Ladder::copy_only(), &src, &dest).unwrap();
        assert_eq!(method, DeployMethod::Copy);

        fs::write(&src, b"two").unwrap();
        assert_eq!(
            fs::read(&dest).unwrap(),
            b"one",
            "copy is independent of source"
        );
    }

    #[test]
    fn probe_reports_the_method_for_a_directory_pair() {
        let dir = tempdir().unwrap();
        let staging = dir.path().join("staging");
        let game = dir.path().join("game");
        let m = probe(&Ladder::default(), &staging, &game).unwrap();
        assert!(matches!(
            m,
            DeployMethod::Reflink | DeployMethod::Hardlink | DeployMethod::Copy
        ));
        assert!(!game.join(".apoc-probe-dest").exists(), "probe cleans up");
    }
}
