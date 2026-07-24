//! The content-addressed original-file vault.
//!
//! Before any pre-existing game file is replaced, its bytes are copied into the
//! vault under their own SHA-256. Rollback restores from here. Content addressing
//! means repeated deploys of the same original cost nothing extra, and a restore
//! can always verify it wrote back exactly what was taken.

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Stream-hash a file with SHA-256, returned as lowercase hex.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// A directory of original files, keyed by content hash.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Vault { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Fan out by the first two hex chars to keep directories small.
    fn key_path(&self, key: &str) -> PathBuf {
        let (prefix, rest) = key.split_at(2.min(key.len()));
        self.root.join(prefix).join(rest)
    }

    /// Store `path`'s bytes and return its vault key. A no-op if already stored.
    pub fn store(&self, path: &Path) -> io::Result<String> {
        let key = hash_file(path)?;
        let dest = self.key_path(&key);
        if dest.exists() {
            return Ok(key);
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        // Write to a temp name then rename, so a crash never leaves a partial
        // object under a hash that claims to be complete.
        let tmp = dest.with_extension("part");
        fs::copy(path, &tmp)?;
        fs::rename(&tmp, &dest)?;
        Ok(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.key_path(key).is_file()
    }

    /// Restore a vaulted original to `dest`, verifying the content hash matches.
    pub fn restore(&self, key: &str, dest: &Path) -> io::Result<()> {
        let src = self.key_path(key);
        if !src.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("vault object {key} is missing"),
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = dest.with_extension("apoc-restore");
        fs::copy(&src, &tmp)?;
        let written = hash_file(&tmp)?;
        if written != key {
            let _ = fs::remove_file(&tmp);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("restored bytes do not match vault key {key}"),
            ));
        }
        fs::rename(&tmp, dest)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stores_and_restores_by_content_hash() {
        let dir = tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault"));

        let original = dir.path().join("original.bin");
        fs::write(&original, b"vanilla bytes").unwrap();
        let key = vault.store(&original).unwrap();
        assert!(vault.contains(&key));

        // Simulate the game file being replaced by a mod.
        fs::write(&original, b"modded bytes").unwrap();
        vault.restore(&key, &original).unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"vanilla bytes");
    }

    #[test]
    fn identical_content_dedupes_to_one_object() {
        let dir = tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault"));
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::write(&a, b"same").unwrap();
        fs::write(&b, b"same").unwrap();
        assert_eq!(vault.store(&a).unwrap(), vault.store(&b).unwrap());
    }

    #[test]
    fn restoring_an_unknown_key_fails_loudly() {
        let dir = tempdir().unwrap();
        let vault = Vault::new(dir.path().join("vault"));
        let dest = dir.path().join("out");
        assert!(vault.restore("deadbeef", &dest).is_err());
        assert!(!dest.exists(), "failed restore must not leave a file behind");
    }
}
