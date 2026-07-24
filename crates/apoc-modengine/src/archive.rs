//! Low-level archive access: read a zip into a flat index + collect `modinfo.ini`
//! contents in one pass, and hash the archive bytes.

use crate::error::Result;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use zip::ZipArchive;

/// A single file entry inside an archive (directories are excluded).
#[derive(Debug, Clone)]
pub struct RawEntry {
    /// Working path, with any redundant wrapper directory stripped. Used for
    /// structure detection and for deriving game-relative destinations.
    pub path: String,
    /// The entry's true name inside the archive, never rewritten. Extraction
    /// must look entries up by this.
    pub archive_path: String,
    /// Uncompressed size in bytes.
    pub size: u64,
}

/// A flat, in-memory view of an archive's contents.
#[derive(Debug, Default)]
pub struct ArchiveIndex {
    pub entries: Vec<RawEntry>,
    /// `(containing_dir, raw_modinfo_text)` in first-seen order, deduped by dir.
    pub modinfos: Vec<(String, String)>,
}

/// Normalize an archive member path: backslashes to `/`, strip a leading `./`.
fn normalize_path(name: &str) -> String {
    let n = name.replace('\\', "/");
    n.strip_prefix("./").unwrap_or(&n).to_string()
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(idx) => path[..idx].to_string(),
        None => String::new(),
    }
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(idx) => &path[idx + 1..],
        None => path,
    }
}

/// Read a zip archive into an [`ArchiveIndex`], capturing `modinfo.ini` bodies.
pub fn read_index(archive_path: &Path) -> Result<ArchiveIndex> {
    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(BufReader::new(file))?;

    let mut index = ArchiveIndex::default();
    let mut seen_modinfo_dirs = std::collections::HashSet::new();

    for i in 0..zip.len() {
        let mut zf = zip.by_index(i)?;
        if zf.is_dir() {
            continue;
        }
        let path = normalize_path(zf.name());
        let size = zf.size();

        if basename(&path).eq_ignore_ascii_case("modinfo.ini") {
            let mut buf = Vec::new();
            zf.read_to_end(&mut buf)?;
            let text = String::from_utf8_lossy(&buf).into_owned();
            let dir = parent_dir(&path);
            if seen_modinfo_dirs.insert(dir.clone()) {
                index.modinfos.push((dir, text));
            }
        }

        index.entries.push(RawEntry {
            archive_path: path.clone(),
            path,
            size,
        });
    }

    Ok(index)
}

/// Stream-hash the archive file with SHA-256, returned as lowercase hex.
pub fn hash_archive(archive_path: &Path) -> Result<String> {
    let mut file = File::open(archive_path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(normalize_path(".\\a\\b.txt"), "a/b.txt");
        assert_eq!(parent_dir("a/b/c.txt"), "a/b");
        assert_eq!(parent_dir("top"), "");
        assert_eq!(basename("a/b/c.txt"), "c.txt");
    }
}
