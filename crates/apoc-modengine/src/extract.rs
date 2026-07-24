//! Staging: extract a mod's payload files into the staging library.
//!
//! Staged mods live **outside the game directory** (`.../games/<game>/staging/<id>/payload/`),
//! namespaced per option so any later selection can be deployed without needing
//! the original archive again:
//!
//! ```text
//! payload/<option_folder>/natives/stm/.../ch03_023_0003.mesh.241111606
//! payload/<option_folder>/reframework/data/.../file.json
//! ```
//!
//! Only payload files (`natives/`, `reframework/`) are extracted; screenshots and
//! `modinfo.ini` are metadata already captured in the [`ModBundle`].

use crate::error::{ModEngineError, Result};
use crate::plan::staged_rel_path;
use apoc_domain::ModBundle;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

/// Outcome of staging a mod archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    pub payload_root: PathBuf,
    pub files_written: usize,
    pub bytes_written: u64,
    /// Preview images extracted alongside the payload.
    pub previews_written: usize,
}

/// Subdirectory of a mod's staging root holding option preview images.
pub const PREVIEW_DIR: &str = ".previews";

/// Staged path of an option's preview image, given its original filename.
pub fn preview_rel_path(option_id: &str, screenshot: &str) -> String {
    let ext = Path::new(screenshot)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    format!("{}/{}.{}", PREVIEW_DIR, crate::plan::option_dir(option_id), ext)
}

/// Locate an option's staged preview image, if one was extracted.
pub fn staged_preview(staging_root: &Path, option_id: &str, screenshot: &str) -> Option<PathBuf> {
    let p = staging_root.join(preview_rel_path(option_id, screenshot));
    p.is_file().then_some(p)
}

/// Read a single entry out of a zip archive. Used to show option previews before
/// a mod has been imported (the wizard runs against the archive directly).
pub fn read_archive_entry(archive_path: &Path, entry_path: &str) -> Result<Vec<u8>> {
    let file = fs::File::open(archive_path)?;
    let mut zip = ZipArchive::new(io::BufReader::new(file))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        if entry.name().replace('\\', "/") == entry_path {
            let mut buf = Vec::with_capacity(entry.size().min(16 * 1024 * 1024) as usize);
            entry.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(ModEngineError::EntryNotFound(entry_path.to_string()))
}

/// Reject absolute paths and `..` traversal (zip-slip) before writing anything.
fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let candidate = Path::new(rel);
    if candidate.is_absolute() {
        return Err(ModEngineError::UnsafePath(rel.to_string()));
    }
    let mut out = root.to_path_buf();
    for comp in candidate.components() {
        match comp {
            Component::Normal(seg) => out.push(seg),
            Component::CurDir => {}
            _ => return Err(ModEngineError::UnsafePath(rel.to_string())),
        }
    }
    if !out.starts_with(root) {
        return Err(ModEngineError::UnsafePath(rel.to_string()));
    }
    Ok(out)
}

/// Extract every deployable payload file of `bundle` from `archive_path` into
/// `dest_root`, laid out as `<dest_root>/<option_folder>/<game_rel_path>`.
///
/// Existing files are overwritten, so re-staging is idempotent.
pub fn stage_bundle(archive_path: &Path, bundle: &ModBundle, dest_root: &Path) -> Result<StageReport> {
    // archive entry path -> staged relative path
    let mut wanted: HashMap<&str, String> = HashMap::new();
    for opt in bundle.deployable_options() {
        for f in &opt.payload {
            wanted.insert(
                f.archive_path.as_str(),
                staged_rel_path(&opt.id, &f.game_rel_path),
            );
        }
    }
    if wanted.is_empty() {
        return Err(ModEngineError::Empty);
    }

    // Preview images for every option that declares one: including info-only
    // options (cover art, warnings), which carry no payload but are the images
    // the wizard most needs to show.
    let mut preview_count = 0usize;
    for opt in bundle.options() {
        if let (Some(shot), Some(path)) = (&opt.screenshot, &opt.screenshot_archive_path) {
            wanted.insert(path.as_str(), preview_rel_path(&opt.id, shot));
            preview_count += 1;
        }
    }
    let _ = preview_count;

    fs::create_dir_all(dest_root)?;
    let file = fs::File::open(archive_path)?;
    let mut zip = ZipArchive::new(io::BufReader::new(file))?;

    let mut files_written = 0usize;
    let mut previews_written = 0usize;
    let mut bytes_written = 0u64;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let Some(rel) = wanted.get(name.as_str()) else {
            continue;
        };
        let out_path = safe_join(dest_root, rel)?;
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)?;
        let n = io::copy(&mut entry, &mut out)?;
        if rel.starts_with(PREVIEW_DIR) {
            previews_written += 1;
        } else {
            files_written += 1;
        }
        bytes_written += n;
    }

    Ok(StageReport {
        payload_root: dest_root.to_path_buf(),
        files_written,
        bytes_written,
        previews_written,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let root = Path::new("/tmp/stage-root");
        assert!(safe_join(root, "../escape").is_err());
        assert!(safe_join(root, "/etc/passwd").is_err());
        assert!(safe_join(root, "a/../../b").is_err());
        assert!(safe_join(root, "opt/natives/ok.mesh.1").is_ok());
    }

    #[test]
    fn preserves_versioned_extensions_verbatim() {
        let root = Path::new("/tmp/stage-root");
        let p = safe_join(root, "Helm-01/natives/stm/x.mesh.241111606").unwrap();
        assert!(p.to_string_lossy().ends_with("x.mesh.241111606"));
    }
}
