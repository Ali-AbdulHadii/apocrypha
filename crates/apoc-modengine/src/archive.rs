//! Low-level archive access.
//!
//! Three container formats reach us from mod sites: zip, 7z and rar. They are
//! not interchangeable at the API level. Zip allows random access, 7z decodes a
//! block at a time, and rar is a strictly forward cursor that must be told to
//! read or skip each entry before it will advance. Rather than pretend they are
//! the same, everything above this module is written against one shape they can
//! all honour: a single forward pass that decides per entry whether the bytes
//! are wanted.
//!
//! That shape also happens to be the efficient one. Skipping an unwanted entry
//! avoids decompressing it in every format, and a mod archive is mostly files
//! the current selection does not need.

use crate::error::{ModEngineError, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
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
    /// The FOMOD manifest, when the archive ships one.
    pub fomod: Option<FomodSource>,
}

/// A `fomod/` directory's documents, captured during the same pass that indexes
/// everything else.
#[derive(Debug, Clone)]
pub struct FomodSource {
    /// Working path of the directory *containing* `fomod/`, empty at the root.
    /// Rewritten alongside [`RawEntry::path`] when a wrapper is stripped.
    pub dir: String,
    /// `ModuleConfig.xml`'s true name inside the archive. Discovery matches
    /// case-insensitively but extraction does not, so the casing found here is
    /// the only one that will ever open the file again.
    pub config_archive_path: String,
    pub config: Vec<u8>,
    /// `info.xml`, when present. Optional metadata; a missing one is not an error.
    pub info: Option<Vec<u8>>,
}

/// Container formats the engine can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    SevenZip,
    Rar,
}

/// Identify a container by its leading bytes.
///
/// Deliberately not by extension. Mod sites are full of archives named for the
/// format the author meant rather than the one their packer produced, and a
/// misnamed file that opens fine is better than a correct refusal.
pub fn detect(archive_path: &Path) -> Result<Format> {
    let mut head = [0u8; 8];
    let read = {
        let mut file = File::open(archive_path)?;
        read_up_to(&mut file, &mut head)?
    };
    let head = &head[..read];

    // Zip covers the local-header, empty-archive and spanned signatures.
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        return Ok(Format::Zip);
    }
    if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Ok(Format::SevenZip);
    }
    // "Rar!\x1a\x07" then 0x00 for RAR4 or 0x01 0x00 for RAR5. Both are handled
    // by the same decoder, so the version is not distinguished here.
    if head.starts_with(b"Rar!\x1a\x07") {
        return Ok(Format::Rar);
    }

    Err(ModEngineError::UnknownFormat(
        archive_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| archive_path.display().to_string()),
    ))
}

/// Read until the buffer is full or the file ends, returning bytes read.
fn read_up_to(file: &mut File, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
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

/// Asked, from the path alone, whether an entry's bytes are needed.
type Want<'a> = &'a mut dyn FnMut(&str) -> bool;

/// Receives every entry with its uncompressed size, and its bytes when wanted.
type OnEntry<'a> = &'a mut dyn FnMut(&str, u64, Option<Vec<u8>>) -> Result<()>;

/// Turns a staged relative path into an absolute destination, rejecting any
/// that would escape the staging root.
type Resolve<'a> = &'a mut dyn FnMut(&str) -> Result<PathBuf>;

/// Walk every file entry in an archive exactly once, in storage order.
///
/// `want` is asked, from the path alone, whether the entry's bytes are needed.
/// Answering false skips decompression entirely. `on_entry` then receives every
/// entry with its uncompressed size, and the bytes only when they were wanted.
///
/// Entries are surfaced with separators already normalized, so callers never
/// see a backslash regardless of which packer produced the archive.
fn scan(archive_path: &Path, want: Want, on_entry: OnEntry) -> Result<()> {
    match detect(archive_path)? {
        Format::Zip => scan_zip(archive_path, want, on_entry),
        Format::SevenZip => scan_7z(archive_path, want, on_entry),
        Format::Rar => scan_rar(archive_path, want, on_entry),
    }
}

fn scan_zip(archive_path: &Path, want: Want, on_entry: OnEntry) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(BufReader::new(file))?;

    for i in 0..zip.len() {
        let mut zf = zip.by_index(i)?;
        if zf.is_dir() {
            continue;
        }
        let path = normalize_path(zf.name());
        let size = zf.size();
        let bytes = if want(&path) {
            let mut buf = Vec::new();
            zf.read_to_end(&mut buf)?;
            Some(buf)
        } else {
            None
        };
        on_entry(&path, size, bytes)?;
    }
    Ok(())
}

fn scan_7z(archive_path: &Path, want: Want, on_entry: OnEntry) -> Result<()> {
    use sevenz_rust2::{ArchiveReader, Password};

    let mut reader = ArchiveReader::open(archive_path, Password::empty())?;
    // The closure cannot return our error type, so a failure inside it is
    // parked here and re-raised once the walk is over.
    let mut failure: Option<ModEngineError> = None;

    reader.for_each_entries(|entry, rd| {
        if entry.is_directory {
            return Ok(true);
        }
        let path = normalize_path(&entry.name);
        let bytes = if want(&path) {
            let mut buf = Vec::new();
            // A solid block must be read through even when the entry is
            // unwanted, so this reads rather than skips; the library handles
            // draining the ones we decline.
            rd.read_to_end(&mut buf)?;
            Some(buf)
        } else {
            None
        };
        if let Err(e) = on_entry(&path, entry.size, bytes) {
            failure = Some(e);
            return Ok(false);
        }
        Ok(true)
    })?;

    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn scan_rar(archive_path: &Path, want: Want, on_entry: OnEntry) -> Result<()> {
    let mut archive = unrar::Archive::new(archive_path).open_for_processing()?;

    // The cursor is a typestate: each step consumes the handle and hands back
    // the next one, so the loop has to thread it through rather than borrow.
    while let Some(at_file) = archive.read_header()? {
        let header = at_file.entry();
        if header.is_directory() {
            archive = at_file.skip()?;
            continue;
        }
        let path = normalize_path(&header.filename.to_string_lossy());
        let size = header.unpacked_size;

        if want(&path) {
            let (bytes, next) = at_file.read()?;
            archive = next;
            on_entry(&path, size, Some(bytes))?;
        } else {
            archive = at_file.skip()?;
            on_entry(&path, size, None)?;
        }
    }
    Ok(())
}

/// Which FOMOD document a path is, if it is one.
///
/// Both the directory and the file name are matched case-insensitively, because
/// `fomod/ModuleConfig.xml`, `FOMOD/moduleconfig.xml` and every casing between
/// them appear in the wild and all of them are the same file to the tools that
/// wrote them. The directory must genuinely be named `fomod`: a stray
/// `info.xml` elsewhere in an archive is somebody's documentation.
fn fomod_doc(path: &str) -> Option<FomodDoc> {
    if !parent_dir(path)
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .eq_ignore_ascii_case("fomod")
    {
        return None;
    }
    let file = basename(path);
    if file.eq_ignore_ascii_case("moduleconfig.xml") {
        Some(FomodDoc::Config)
    } else if file.eq_ignore_ascii_case("info.xml") {
        Some(FomodDoc::Info)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FomodDoc {
    Config,
    Info,
}

/// How deep in the archive a path sits, used to prefer the outermost manifest.
fn depth(path: &str) -> usize {
    path.matches('/').count()
}

/// Read an archive into an [`ArchiveIndex`], capturing `modinfo.ini` bodies and
/// any FOMOD manifest.
///
/// Both are collected in the **same** forward pass. Rar is a cursor that cannot
/// be rewound, so a second walk would mean opening the archive again, and the
/// module's whole shape exists to avoid that.
pub fn read_index(archive_path: &Path) -> Result<ArchiveIndex> {
    let mut index = ArchiveIndex::default();
    let mut seen_modinfo_dirs = std::collections::HashSet::new();
    // Kept separately from `index.fomod` because config and info arrive in
    // whichever order the archive stored them, and either may be missing.
    let mut config: Option<(String, Vec<u8>)> = None;
    let mut info: Option<(String, Vec<u8>)> = None;

    scan(
        archive_path,
        &mut |path| basename(path).eq_ignore_ascii_case("modinfo.ini") || fomod_doc(path).is_some(),
        &mut |path, size, bytes| {
            if let Some(buf) = bytes {
                match fomod_doc(path) {
                    Some(FomodDoc::Config) => {
                        // An archive containing two FOMODs is a repack, and
                        // merging two installers would be inventing a third.
                        // The outermost one is the one the repacker meant.
                        if config
                            .as_ref()
                            .map_or(true, |(p, _)| depth(path) < depth(p))
                        {
                            config = Some((path.to_string(), buf));
                        }
                    }
                    Some(FomodDoc::Info) => {
                        if info.as_ref().map_or(true, |(p, _)| depth(path) < depth(p)) {
                            info = Some((path.to_string(), buf));
                        }
                    }
                    None => {
                        let text = String::from_utf8_lossy(&buf).into_owned();
                        let dir = parent_dir(path);
                        if seen_modinfo_dirs.insert(dir.clone()) {
                            index.modinfos.push((dir, text));
                        }
                    }
                }
            }
            index.entries.push(RawEntry {
                archive_path: path.to_string(),
                path: path.to_string(),
                size,
            });
            Ok(())
        },
    )?;

    // `info.xml` alone is not a FOMOD. The manifest is what declares the
    // installer, and without it there is nothing to install from.
    if let Some((config_path, config_bytes)) = config {
        let fomod_dir = parent_dir(&config_path);
        index.fomod = Some(FomodSource {
            dir: parent_dir(&fomod_dir),
            config_archive_path: config_path,
            config: config_bytes,
            // Only an `info.xml` belonging to the same `fomod/` directory.
            info: info
                .filter(|(p, _)| parent_dir(p) == fomod_dir)
                .map(|(_, b)| b),
        });
    }

    Ok(index)
}

/// Read a single entry out of an archive. Used to show option previews before
/// a mod has been imported (the wizard runs against the archive directly).
pub fn read_entry(archive_path: &Path, entry_path: &str) -> Result<Vec<u8>> {
    let mut found: Option<Vec<u8>> = None;
    scan(
        archive_path,
        // Matching on the path alone, with no reference to what has been found
        // so far: the two closures run against the same walk and cannot both
        // hold the result.
        &mut |path| path == entry_path,
        &mut |_, _, bytes| {
            if let Some(buf) = bytes {
                found = Some(buf);
            }
            Ok(())
        },
    )?;
    found.ok_or_else(|| ModEngineError::EntryNotFound(entry_path.to_string()))
}

/// Extract the named entries to disk.
///
/// `wanted` maps an archive entry path to **every** staged relative path that
/// wants its bytes; `resolve` turns each into an absolute destination and is
/// where traversal is rejected. Returns `(staged_rel_path, bytes)` per file
/// written.
///
/// One entry to many destinations, rather than one to one. Under the folder
/// conventions this engine grew up with, no archive entry could ever belong to
/// two options: each option owned a directory. A manifest-driven installer
/// names its sources instead, so two options can cite the same texture, and one
/// option can map one file to two places. With a single destination per entry
/// the second claim silently replaced the first and an option staged short,
/// with nothing anywhere reporting it.
///
/// This does not go through [`scan`], because scan hands entries over as a
/// `Vec<u8>` and a texture pack should not be held in memory to be copied to
/// disk. Each format streams by its own best route instead.
pub fn extract_entries(
    archive_path: &Path,
    wanted: &HashMap<String, Vec<String>>,
    resolve: Resolve,
) -> Result<Vec<(String, u64)>> {
    match detect(archive_path)? {
        Format::Zip => extract_zip(archive_path, wanted, resolve),
        Format::SevenZip => extract_7z(archive_path, wanted, resolve),
        Format::Rar => extract_rar(archive_path, wanted, resolve),
    }
}

/// Create the parent directory of a destination that is about to be written.
fn prepare(out_path: &Path) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Copy an already-written destination to the rest that wanted the same entry.
///
/// Decompressing once and copying is cheaper than decoding the entry again, and
/// for rar it is the only option: the cursor has already moved on.
fn fan_out(first: &Path, rest: &[String], resolve: Resolve) -> Result<Vec<(String, u64)>> {
    let mut written = Vec::new();
    for rel in rest {
        let out_path = resolve(rel)?;
        prepare(&out_path)?;
        let n = std::fs::copy(first, &out_path)?;
        written.push((rel.clone(), n));
    }
    Ok(written)
}

fn extract_zip(
    archive_path: &Path,
    wanted: &HashMap<String, Vec<String>>,
    resolve: Resolve,
) -> Result<Vec<(String, u64)>> {
    let file = File::open(archive_path)?;
    let mut zip = ZipArchive::new(BufReader::new(file))?;
    let mut written = Vec::new();

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = normalize_path(entry.name());
        let Some(rels) = wanted.get(name.as_str()) else {
            continue;
        };
        let Some((first, rest)) = rels.split_first() else {
            continue;
        };
        let out_path = resolve(first)?;
        prepare(&out_path)?;
        let mut out = File::create(&out_path)?;
        let n = std::io::copy(&mut entry, &mut out)?;
        written.push((first.clone(), n));
        drop(out);
        written.extend(fan_out(&out_path, rest, resolve)?);
    }
    Ok(written)
}

fn extract_7z(
    archive_path: &Path,
    wanted: &HashMap<String, Vec<String>>,
    resolve: Resolve,
) -> Result<Vec<(String, u64)>> {
    use sevenz_rust2::{ArchiveReader, Password};

    let mut reader = ArchiveReader::open(archive_path, Password::empty())?;
    let mut written = Vec::new();
    // The library's callback returns its own error type, so ours is parked and
    // re-raised after the walk rather than being flattened into a 7z error.
    let mut failure: Option<ModEngineError> = None;

    reader.for_each_entries(|entry, rd| {
        if entry.is_directory {
            return Ok(true);
        }
        let name = normalize_path(&entry.name);
        let Some(rels) = wanted.get(name.as_str()) else {
            return Ok(true);
        };
        let Some((first, rest)) = rels.split_first() else {
            return Ok(true);
        };
        let result = (|| -> Result<Vec<(String, u64)>> {
            let out_path = resolve(first)?;
            prepare(&out_path)?;
            let mut out = File::create(&out_path)?;
            let n = std::io::copy(rd, &mut out)?;
            drop(out);
            let mut done = vec![(first.clone(), n)];
            done.extend(fan_out(&out_path, rest, resolve)?);
            Ok(done)
        })();
        match result {
            Ok(done) => {
                written.extend(done);
                Ok(true)
            }
            Err(e) => {
                failure = Some(e);
                Ok(false)
            }
        }
    })?;

    match failure {
        Some(e) => Err(e),
        None => Ok(written),
    }
}

fn extract_rar(
    archive_path: &Path,
    wanted: &HashMap<String, Vec<String>>,
    resolve: Resolve,
) -> Result<Vec<(String, u64)>> {
    let mut archive = unrar::Archive::new(archive_path).open_for_processing()?;
    let mut written = Vec::new();

    while let Some(at_file) = archive.read_header()? {
        // Copied out before the cursor is consumed: reading or skipping takes
        // ownership, and the header borrows from it.
        let (name, size, is_dir) = {
            let h = at_file.entry();
            (
                normalize_path(&h.filename.to_string_lossy()),
                h.unpacked_size,
                h.is_directory(),
            )
        };

        if is_dir {
            archive = at_file.skip()?;
            continue;
        }
        match wanted.get(name.as_str()).and_then(|r| r.split_first()) {
            Some((first, rest)) => {
                let out_path = resolve(first)?;
                prepare(&out_path)?;
                // unrar writes the file itself, so nothing is buffered.
                archive = at_file.extract_to(&out_path)?;
                written.push((first.clone(), size));
                // The cursor has moved past this entry and cannot go back, so
                // the remaining destinations are copies of what was just
                // written rather than a second extraction.
                written.extend(fan_out(&out_path, rest, resolve)?);
            }
            None => archive = at_file.skip()?,
        }
    }
    Ok(written)
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
    use std::io::Write;

    #[test]
    fn path_helpers() {
        assert_eq!(normalize_path(".\\a\\b.txt"), "a/b.txt");
        assert_eq!(parent_dir("a/b/c.txt"), "a/b");
        assert_eq!(parent_dir("top"), "");
        assert_eq!(basename("a/b/c.txt"), "c.txt");
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.join(name);
        let mut f = File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

    #[test]
    fn format_comes_from_the_bytes_not_the_extension() {
        let dir = tempfile::tempdir().unwrap();

        // A rar named .zip must still be recognised as a rar: mod sites are
        // full of archives named for the format the author meant.
        let liar = write(
            dir.path(),
            "actually-a-rar.zip",
            b"Rar!\x1a\x07\x01\x00rest",
        );
        assert_eq!(detect(&liar).unwrap(), Format::Rar);

        let z = write(dir.path(), "real.zip", b"PK\x03\x04rest");
        assert_eq!(detect(&z).unwrap(), Format::Zip);

        let s = write(dir.path(), "real.7z", b"7z\xbc\xaf\x27\x1crest");
        assert_eq!(detect(&s).unwrap(), Format::SevenZip);

        let rar4 = write(dir.path(), "old.rar", b"Rar!\x1a\x07\x00rest");
        assert_eq!(detect(&rar4).unwrap(), Format::Rar);
    }

    #[test]
    fn a_non_archive_is_named_in_the_error() {
        let dir = tempfile::tempdir().unwrap();
        let junk = write(dir.path(), "notes.txt", b"just some text");
        let err = detect(&junk).unwrap_err().to_string();
        assert!(err.contains("notes.txt"), "{err}");
    }

    #[test]
    fn a_truncated_file_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let tiny = write(dir.path(), "tiny.zip", b"PK");
        assert!(detect(&tiny).is_err());
        let empty = write(dir.path(), "empty.zip", b"");
        assert!(detect(&empty).is_err());
    }
}
