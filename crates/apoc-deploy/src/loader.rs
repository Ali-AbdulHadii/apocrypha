//! Proton loader provisioning: the reason Linux modding usually fails.
//!
//! A DLL-proxy loader (REFramework's `dinput8.dll`) only runs under Proton if
//! Wine is told to prefer the native DLL over its builtin. We register that in
//! the prefix registry (`pfx/user.reg`), which is durable and needs no Steam UI,
//! and additionally surface the equivalent launch-option string for the user.
//!
//! Safety: `user.reg` is only ever rewritten via a temp-file + rename, the prior
//! value is captured for rollback, and Steam must not be running when we touch it.

use std::fs;
use std::io;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

const OVERRIDES_SECTION: &str = r"[Software\\Wine\\DllOverrides]";

/// Current state of a prefix's DLL override for one DLL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideState {
    pub name: String,
    pub value: Option<String>,
}

/// Read the current override value for `dll_name` (e.g. `dinput8`) from `user.reg`.
pub fn read_override(user_reg: &Path, dll_name: &str) -> io::Result<OverrideState> {
    let value = match fs::read_to_string(user_reg) {
        Ok(text) => find_override(&text, dll_name),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    Ok(OverrideState {
        name: dll_name.to_string(),
        value,
    })
}

fn find_override(text: &str, dll_name: &str) -> Option<String> {
    let needle = format!("\"{dll_name}\"=");
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed.starts_with(OVERRIDES_SECTION.trim_end_matches(']'))
                || trimmed.eq_ignore_ascii_case(OVERRIDES_SECTION);
            continue;
        }
        if in_section && trimmed.starts_with(&needle) {
            return trimmed
                .split_once('=')
                .map(|(_, v)| v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Write (or replace) the DLL override in `user.reg`, returning the previous value.
///
/// Creates the `[Software\Wine\DllOverrides]` section if absent. The file is
/// written atomically; on any failure the original file is left untouched.
pub fn write_override(user_reg: &Path, dll_name: &str, value: &str) -> io::Result<Option<String>> {
    let existing = match fs::read_to_string(user_reg) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let previous = find_override(&existing, dll_name);
    let updated = upsert_override(&existing, dll_name, value);

    if let Some(parent) = user_reg.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = user_reg.with_extension("reg.apoc-tmp");
    fs::write(&tmp, updated)?;
    fs::rename(&tmp, user_reg)?;
    Ok(previous)
}

/// Remove our override, or restore a previous value if there was one.
pub fn restore_override(user_reg: &Path, dll_name: &str, previous: Option<&str>) -> io::Result<()> {
    let existing = match fs::read_to_string(user_reg) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let updated = match previous {
        Some(prev) => upsert_override(&existing, dll_name, prev),
        None => remove_override(&existing, dll_name),
    };
    let tmp = user_reg.with_extension("reg.apoc-tmp");
    fs::write(&tmp, updated)?;
    fs::rename(&tmp, user_reg)?;
    Ok(())
}

fn section_header_matches(line: &str) -> bool {
    let t = line.trim();
    t.eq_ignore_ascii_case(OVERRIDES_SECTION)
        || t.to_ascii_lowercase()
            .starts_with(&OVERRIDES_SECTION.to_ascii_lowercase())
}

fn upsert_override(text: &str, dll_name: &str, value: &str) -> String {
    let entry = format!("\"{dll_name}\"=\"{value}\"");
    let needle = format!("\"{dll_name}\"=");

    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut wrote = false;
    let mut saw_section = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Leaving the overrides section without having written: append here.
            if in_section && !wrote {
                out.push(entry.clone());
                wrote = true;
            }
            in_section = section_header_matches(trimmed);
            saw_section |= in_section;
            out.push(line.to_string());
            continue;
        }
        if in_section && trimmed.starts_with(&needle) {
            out.push(entry.clone());
            wrote = true;
            continue;
        }
        out.push(line.to_string());
    }

    if in_section && !wrote {
        out.push(entry.clone());
        wrote = true;
    }
    if !saw_section {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(OVERRIDES_SECTION.to_string());
        out.push(entry);
    } else if !wrote {
        out.push(entry);
    }

    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn remove_override(text: &str, dll_name: &str) -> String {
    let needle = format!("\"{dll_name}\"=");
    let mut out = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = section_header_matches(trimmed);
            out.push(line.to_string());
            continue;
        }
        if in_section && trimmed.starts_with(&needle) {
            continue;
        }
        out.push(line.to_string());
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Is Steam currently running? Editing prefix/config files under a live Steam is
/// unsafe, so callers refuse rather than risk clobbering.
///
/// Implemented per platform rather than left to fail. The `/proc` walk below
/// simply returns `false` on Windows, where there is no `/proc` — so a check
/// whose entire purpose is to refuse would have quietly started permitting
/// everything, on the platform where nobody had tested it. A safety check that
/// degrades to "yes, go ahead" is worse than no check, because callers still
/// read as though it happened.
#[cfg(unix)]
pub fn steam_is_running() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else { continue };
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let comm = PathBuf::from("/proc").join(pid).join("comm");
        if let Ok(text) = fs::read_to_string(&comm) {
            if text.trim() == "steam" {
                return true;
            }
        }
    }
    false
}

/// The same question on Windows, asked of `tasklist`.
///
/// A spawned process rather than a process-enumeration API, because the latter
/// means a Windows-only dependency and unsafe calls for one boolean. It costs
/// tens of milliseconds and is only asked before an apply and on an explicit
/// status request, never in a loop.
///
/// Unreachable output — tasklist missing, refused, or unparsable — reads as
/// "running". The refusal is the safe answer: the worst case is telling someone
/// to close a Steam that is already closed, against a corrupted install if the
/// check were wrong the other way.
#[cfg(windows)]
pub fn steam_is_running() -> bool {
    use std::process::Command;

    let output = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
            // tasklist prints an informational line rather than failing when
            // nothing matches, so the process name is what has to be found.
            text.contains("steam.exe")
        }
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_the_section_when_absent() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("user.reg");
        fs::write(&reg, "WINE REGISTRY Version 2\n\n[Software\\\\Wine]\n").unwrap();

        let prev = write_override(&reg, "dinput8", "native,builtin").unwrap();
        assert_eq!(prev, None);

        let state = read_override(&reg, "dinput8").unwrap();
        assert_eq!(state.value.as_deref(), Some("native,builtin"));
    }

    #[test]
    fn replaces_an_existing_value_and_reports_the_previous() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("user.reg");
        write_override(&reg, "dinput8", "builtin").unwrap();

        let prev = write_override(&reg, "dinput8", "native,builtin").unwrap();
        assert_eq!(prev.as_deref(), Some("builtin"));
        assert_eq!(
            read_override(&reg, "dinput8").unwrap().value.as_deref(),
            Some("native,builtin")
        );
    }

    #[test]
    fn rollback_removes_ours_or_restores_theirs() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("user.reg");

        // No prior value -> rollback removes the entry entirely.
        write_override(&reg, "dinput8", "native,builtin").unwrap();
        restore_override(&reg, "dinput8", None).unwrap();
        assert_eq!(read_override(&reg, "dinput8").unwrap().value, None);

        // Prior value -> rollback restores it.
        write_override(&reg, "dinput8", "builtin").unwrap();
        let prev = write_override(&reg, "dinput8", "native,builtin").unwrap();
        restore_override(&reg, "dinput8", prev.as_deref()).unwrap();
        assert_eq!(
            read_override(&reg, "dinput8").unwrap().value.as_deref(),
            Some("builtin")
        );
    }

    #[test]
    fn missing_user_reg_reads_as_no_override() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("nonexistent/user.reg");
        assert_eq!(read_override(&reg, "dinput8").unwrap().value, None);
    }

    #[test]
    fn other_dll_overrides_are_left_alone() {
        let dir = tempdir().unwrap();
        let reg = dir.path().join("user.reg");
        write_override(&reg, "dsound", "native").unwrap();
        write_override(&reg, "dinput8", "native,builtin").unwrap();
        restore_override(&reg, "dinput8", None).unwrap();
        assert_eq!(
            read_override(&reg, "dsound").unwrap().value.as_deref(),
            Some("native"),
            "unrelated overrides must survive our rollback"
        );
    }
}
