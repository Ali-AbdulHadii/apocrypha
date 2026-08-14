//! Download queue.
//!
//! Downloads run on their own threads and report progress through events, so a
//! large archive never blocks the interface and never forces a dialog open the
//! moment it lands. A finished download waits in the queue until the user
//! chooses to install it.
//!
//! The queue also lists archives it did not download. Anything sitting in the
//! downloads folder is offered for install alongside the rest, which is how a
//! free Nexus account works in practice: the user saves the file from their
//! browser, and it should not matter that Apocrypha was not the one to fetch it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Extensions worth listing. The engine reads all three, and identifies the
/// real format from the file's leading bytes rather than trusting the name, so
/// this is only a filter for what to show, never a claim about contents.
const ARCHIVE_EXT: &[&str] = &["zip", "7z", "rar"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DownloadState {
    Downloading,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub file_name: String,
    pub path: String,
    /// `None` while the server has not told us the size.
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    pub state: DownloadState,
    #[serde(default)]
    pub error: Option<String>,
    /// Where it came from, for the interface. "Nexus Mods" or "Downloads folder".
    pub source: String,
    /// Unix seconds when it started, used for ordering and for speed.
    pub started_at: u64,
    /// Bytes per second, averaged over the transfer so far.
    pub bytes_per_second: u64,
    /// Name of the mod this archive was imported as, when it has been.
    ///
    /// Filled in by the command layer rather than the queue, which knows
    /// nothing about the library. Absent on a progress event, where the
    /// question cannot yet apply.
    #[serde(default)]
    pub installed_as: Option<String>,
    /// Name of the mod this archive was fetched to update, when it was fetched
    /// from the update screen.
    ///
    /// Filled in by the command layer from recorded provenance, like
    /// `installed_as` and for the same reason: the queue knows nothing about the
    /// library, and nothing here should teach it.
    #[serde(default)]
    pub update_for: Option<String>,
}

#[derive(Default)]
pub struct Queue {
    items: Mutex<HashMap<String, Download>>,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("dl-{nanos:x}")
}

/// Keep a server-supplied name to one safe path segment.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "download.bin".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_archive(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| ARCHIVE_EXT.iter().any(|a| a.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

/// Outcome of registering a download.
pub enum Begin {
    /// Registered. The caller owns driving the transfer.
    Started(Download),
    /// A transfer for this file was already running. Nothing was registered.
    AlreadyRunning(Download),
}

impl Queue {
    /// Register a download before its thread starts, so the interface can show
    /// it immediately rather than after the first byte arrives.
    ///
    /// Refuses to register a second transfer for a destination one is already
    /// writing. That is a correctness guard, not tidiness: two threads sharing
    /// a path truncate each other's part file, and whichever renames first
    /// leaves the loser appending into the finished archive. Two ways in hit
    /// this, a duplicated event listener and a double press of Mod Manager
    /// Download, so it is enforced here where the lock is held once rather
    /// than by checking before calling.
    pub fn begin(&self, file_name: &str, path: &Path, source: &str) -> Begin {
        let dest = path.display().to_string();

        // Check and insert under one guard. Splitting them would leave a window
        // where two callers both see nothing running and both proceed, which is
        // the exact race this exists to prevent.
        let d = {
            // Poisoning only means some other thread panicked; the map itself
            // is still a valid map, and refusing every future download because
            // of it would be worse than carrying on.
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(running) = items
                .values()
                .find(|d| d.state == DownloadState::Downloading && d.path == dest)
            {
                return Begin::AlreadyRunning(running.clone());
            }
            let d = Download {
                id: new_id(),
                file_name: file_name.to_string(),
                path: dest,
                total_bytes: None,
                received_bytes: 0,
                state: DownloadState::Downloading,
                error: None,
                source: source.to_string(),
                started_at: now(),
                bytes_per_second: 0,
                installed_as: None,
                update_for: None,
            };
            items.insert(d.id.clone(), d.clone());
            d
        };

        if let Ok(mut c) = self.cancels.lock() {
            c.insert(d.id.clone(), Arc::new(AtomicBool::new(false)));
        }
        Begin::Started(d)
    }

    pub fn cancel_flag(&self, id: &str) -> Option<Arc<AtomicBool>> {
        self.cancels.lock().ok()?.get(id).cloned()
    }

    pub fn request_cancel(&self, id: &str) {
        if let Some(f) = self.cancel_flag(id) {
            f.store(true, Ordering::Relaxed);
        }
    }

    pub fn update<F: FnOnce(&mut Download)>(&self, id: &str, f: F) -> Option<Download> {
        let mut items = self.items.lock().ok()?;
        let d = items.get_mut(id)?;
        f(d);
        Some(d.clone())
    }

    pub fn get(&self, id: &str) -> Option<Download> {
        self.items.lock().ok()?.get(id).cloned()
    }

    pub fn forget(&self, id: &str) {
        if let Ok(mut items) = self.items.lock() {
            items.remove(id);
        }
        if let Ok(mut c) = self.cancels.lock() {
            c.remove(id);
        }
    }

    /// Everything tracked, plus any archive in `dir` that is not tracked yet.
    ///
    /// Scanning means a file saved from a browser appears in the queue without
    /// the user having to find it, which is the normal path for a free account.
    pub fn list(&self, dir: &Path) -> Vec<Download> {
        let mut out: Vec<Download> = self
            .items
            .lock()
            .map(|i| i.values().cloned().collect())
            .unwrap_or_default();

        let known: Vec<String> = out.iter().map(|d| d.path.clone()).collect();

        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_file() || !is_archive(&p) {
                    continue;
                }
                let path = p.display().to_string();
                if known.contains(&path) {
                    continue;
                }
                let meta = e.metadata().ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                out.push(Download {
                    // Derived from the path so the id is stable across restarts.
                    id: format!("file-{:x}", hash_str(&path)),
                    file_name: p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("archive")
                        .to_string(),
                    path,
                    total_bytes: Some(size),
                    received_bytes: size,
                    state: DownloadState::Ready,
                    error: None,
                    source: "Downloads folder".into(),
                    started_at: mtime,
                    bytes_per_second: 0,
                    installed_as: None,
                    update_for: None,
                });
            }
        }

        out.sort_by_key(|d| std::cmp::Reverse(d.started_at));
        out
    }
}

fn hash_str(s: &str) -> u64 {
    // Small stable hash. Only needs to be consistent, not cryptographic.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Progress callback: called with the download after each chunk.
pub type OnProgress = Box<dyn Fn(&Download) + Send>;

/// Fetch `url` into `dest`, reporting progress and honouring cancellation.
///
/// Writes to a `.part` file and renames on success, so a cancelled or failed
/// transfer never leaves something that looks like a complete archive.
pub fn fetch(
    queue: &Queue,
    id: &str,
    url: &str,
    dest: &Path,
    on_progress: &OnProgress,
) -> Result<u64, String> {
    let cancel = queue.cancel_flag(id);
    let res = ureq::get(url).call().map_err(|e| e.to_string())?;

    let total: Option<u64> = res
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(d) = queue.update(id, |d| d.total_bytes = total) {
        on_progress(&d);
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut reader = res.into_reader();
    let mut buf = [0u8; 128 * 1024];
    let mut received: u64 = 0;
    let started = std::time::Instant::now();
    let mut last_emit = std::time::Instant::now();

    loop {
        if cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err("cancelled".into());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;

        // Emitting on every chunk would flood the interface, so this is capped
        // at roughly five updates a second.
        if last_emit.elapsed().as_millis() >= 200 {
            last_emit = std::time::Instant::now();
            let secs = started.elapsed().as_secs_f64().max(0.001);
            if let Some(d) = queue.update(id, |d| {
                d.received_bytes = received;
                d.bytes_per_second = (received as f64 / secs) as u64;
            }) {
                on_progress(&d);
            }
        }
    }

    drop(file);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;

    let secs = started.elapsed().as_secs_f64().max(0.001);
    if let Some(d) = queue.update(id, |d| {
        d.received_bytes = received;
        d.total_bytes = d.total_bytes.or(Some(received));
        d.bytes_per_second = (received as f64 / secs) as u64;
        d.state = DownloadState::Ready;
    }) {
        on_progress(&d);
    }
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Register a transfer, asserting it was actually new.
    fn started(q: &Queue, name: &str, path: &Path) -> Download {
        match q.begin(name, path, "test") {
            Begin::Started(d) => d,
            Begin::AlreadyRunning(_) => panic!("expected a fresh registration"),
        }
    }

    #[test]
    fn server_names_cannot_escape_the_folder() {
        // The invariant is that the result is one path segment, so joining it
        // onto the downloads folder cannot land anywhere else. Separators are
        // replaced rather than removed, so a `..` left in the middle is inert.
        for name in ["../../etc/passwd", "a/b.zip", "..\\..\\x", "   ", "."] {
            let s = safe_name(name);
            assert!(!s.contains('/'), "{name} -> {s}");
            assert!(!s.contains('\\'), "{name} -> {s}");
            assert!(s != "." && s != "..", "{name} -> {s}");
            assert_eq!(Path::new(&s).components().count(), 1, "{name} -> {s}");
        }
        assert_eq!(safe_name("a/b.zip"), "a_b.zip");
        assert_eq!(safe_name("   "), "download.bin");
        assert_eq!(
            safe_name("Mod-1-2.zip"),
            "Mod-1-2.zip",
            "ordinary names pass through"
        );
    }

    #[test]
    fn only_archives_are_listed() {
        assert!(is_archive(Path::new("a.zip")));
        assert!(is_archive(Path::new("a.7z")));
        assert!(is_archive(Path::new("A.RAR")));
        assert!(!is_archive(Path::new("notes.txt")));
        assert!(!is_archive(Path::new("half.zip.part")));
    }

    #[test]
    fn a_new_entry_has_no_size_until_the_server_reports_one() {
        let q = Queue::default();
        let d = started(&q, "x.zip", Path::new("/tmp/x.zip"));
        assert_eq!(d.total_bytes, None);
        assert_eq!(d.state, DownloadState::Downloading);
        let d = q.update(&d.id, |d| d.total_bytes = Some(200)).unwrap();
        assert_eq!(d.total_bytes, Some(200));
        assert_eq!(q.get(&d.id).unwrap().total_bytes, Some(200));
    }

    #[test]
    fn cancelling_sets_the_flag_the_transfer_watches() {
        let q = Queue::default();
        let d = started(&q, "x.zip", Path::new("/tmp/x.zip"));
        assert!(!q.cancel_flag(&d.id).unwrap().load(Ordering::Relaxed));
        q.request_cancel(&d.id);
        assert!(q.cancel_flag(&d.id).unwrap().load(Ordering::Relaxed));
    }

    #[test]
    fn scanning_finds_untracked_archives_with_stable_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SomeMod.zip"), b"x").unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"x").unwrap();
        let q = Queue::default();

        let a = q.list(dir.path());
        assert_eq!(a.len(), 1, "only the archive is offered");
        assert_eq!(a[0].file_name, "SomeMod.zip");
        assert_eq!(a[0].state, DownloadState::Ready);

        // The id must not change between listings, or the interface loses track.
        let b = q.list(dir.path());
        assert_eq!(a[0].id, b[0].id);
    }

    #[test]
    fn a_second_transfer_for_the_same_file_is_refused() {
        let q = Queue::default();
        let path = Path::new("/tmp/Mod.zip");
        let first = started(&q, "Mod.zip", path);

        // Two threads writing one path would truncate each other's part file,
        // so the second caller is handed the running entry instead.
        match q.begin("Mod.zip", path, "test") {
            Begin::AlreadyRunning(d) => assert_eq!(d.id, first.id),
            Begin::Started(_) => panic!("started a second writer for one file"),
        }
        assert_eq!(q.list(Path::new("/nonexistent")).len(), 1);

        // Once it is no longer running the file may be fetched again, which is
        // what re-downloading a broken archive needs.
        q.update(&first.id, |d| d.state = DownloadState::Failed);
        started(&q, "Mod.zip", path);
    }

    #[test]
    fn tracked_downloads_are_not_duplicated_by_the_scan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Tracked.zip");
        std::fs::write(&path, b"x").unwrap();
        let q = Queue::default();
        started(&q, "Tracked.zip", &path);
        assert_eq!(q.list(dir.path()).len(), 1);
    }
}
