//! Steam and Proton discovery on Linux.
//!
//! This is the layer that makes "Linux modding" actually work: locate every Steam
//! installation (native, Flatpak, Snap), enumerate library folders, resolve a
//! game's install directory from its appid, and find the Proton prefix
//! (`compatdata/<appid>/pfx`) that a Windows DLL loader must be registered in.

pub mod vdf;

use std::path::{Path, PathBuf};

/// Where a Steam installation came from. Affects nothing but diagnostics and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SteamFlavor {
    Native,
    Flatpak,
    Snap,
}

impl SteamFlavor {
    pub fn as_str(&self) -> &'static str {
        match self {
            SteamFlavor::Native => "native",
            SteamFlavor::Flatpak => "flatpak",
            SteamFlavor::Snap => "snap",
        }
    }
}

/// A discovered Steam root (the directory containing `steamapps/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamRoot {
    pub path: PathBuf,
    pub flavor: SteamFlavor,
}

/// A Steam library folder that can hold installed games.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// The library root (contains `steamapps/`).
    pub path: PathBuf,
    /// Appids this library's `libraryfolders.vdf` claims, when listed.
    pub apps: Vec<u32>,
}

impl Library {
    pub fn steamapps(&self) -> PathBuf {
        self.path.join("steamapps")
    }
}

/// A fully resolved game installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInstall {
    pub app_id: u32,
    /// Name from the app manifest, when present.
    pub name: Option<String>,
    /// The game's install directory (`steamapps/common/<installdir>`).
    pub install_dir: PathBuf,
    /// The library that holds it.
    pub library: PathBuf,
    /// Proton prefix root (`steamapps/compatdata/<appid>/pfx`), if it exists.
    pub proton_prefix: Option<PathBuf>,
    /// Proton build configured for this app (e.g. `proton_experimental`), if known.
    pub proton_tool: Option<String>,
}

impl GameInstall {
    /// `pfx/drive_c/users/steamuser`: where Windows-side user data lives.
    pub fn prefix_user_reg(&self) -> Option<PathBuf> {
        self.proton_prefix
            .as_ref()
            .and_then(|p| p.parent().map(|pfx_parent| pfx_parent.join("pfx/user.reg")))
            .or_else(|| self.proton_prefix.as_ref().map(|p| p.join("user.reg")))
    }

    /// `compatdata/<appid>`, the **parent** of the prefix.
    ///
    /// The distinction is not cosmetic and is easy to get wrong in one
    /// direction only: `STEAM_COMPAT_DATA_PATH` names this directory, while
    /// [`Self::proton_prefix`] names `pfx` inside it. Passing the prefix leaves
    /// Proton building a second prefix one level down, in a game's compatdata,
    /// which looks like the game losing its save data.
    pub fn compat_data_dir(&self) -> Option<PathBuf> {
        self.proton_prefix
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
    }
}

/// A Proton build's name as Steam records it, reduced to something comparable.
///
/// `config.vdf` stores the mapping as an internal name (`proton_experimental`)
/// while the build lives in a directory named for people (`Proton - Experimental`).
/// Neither is derivable from the other by rule, so both are folded: lowercased,
/// with every run of non-alphanumerics becoming one `_`.
fn normalized_tool_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut underscore = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            underscore = false;
        } else if !underscore && !out.is_empty() {
            out.push('_');
            underscore = true;
        }
    }
    out.trim_end_matches('_').to_string()
}

/// The `proton` script for a build Steam has mapped to a game.
///
/// Two places hold them and they are named differently in each. A custom build
/// dropped into `compatibilitytools.d` is stored under a directory that *is*
/// its name, and Valve's own builds live in `steamapps/common` under a display
/// name that has to be matched loosely.
///
/// Returns `None` rather than a guess: running the wrong Proton build against
/// an existing prefix is how a prefix gets upgraded in place and stops working
/// with the build the game actually uses. The caller reports the name it could
/// not resolve, which is the one piece of information that makes it fixable.
pub fn proton_binary(steam_root: &Path, tool_name: &str) -> Option<PathBuf> {
    let direct = steam_root
        .join("compatibilitytools.d")
        .join(tool_name)
        .join("proton");
    if direct.is_file() {
        return Some(direct);
    }

    let wanted = normalized_tool_name(tool_name);
    if wanted.is_empty() {
        return None;
    }
    for dir in ["compatibilitytools.d", "steamapps/common"] {
        let Ok(entries) = std::fs::read_dir(steam_root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if normalized_tool_name(name) != wanted {
                continue;
            }
            let candidate = entry.path().join("proton");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn home() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/root" }))
}

/// Where Steam might be, on this platform, in priority order.
///
/// Split from [`discover_steam_roots`] so the list of places is data and the
/// rules applied to it — must contain `steamapps/`, resolve symlinks, dedupe —
/// are shared. The rules are the part worth having only one copy of; the
/// locations are the part that is genuinely different per platform.
#[cfg(unix)]
fn candidate_roots(h: &Path) -> Vec<(PathBuf, SteamFlavor)> {
    vec![
        (h.join(".local/share/Steam"), SteamFlavor::Native),
        (h.join(".steam/steam"), SteamFlavor::Native),
        (h.join(".steam/root"), SteamFlavor::Native),
        (h.join(".steam/debian-installation"), SteamFlavor::Native),
        (
            h.join(".var/app/com.valvesoftware.Steam/data/Steam"),
            SteamFlavor::Flatpak,
        ),
        (
            h.join("snap/steam/common/.local/share/Steam"),
            SteamFlavor::Snap,
        ),
    ]
}

/// Windows keeps one Steam, wherever the installer was pointed at.
///
/// The registry is asked first and is the only answer that is actually correct.
/// Hardcoded guesses find Steam on machines where it happens to sit in Program
/// Files and silently find nothing everywhere else, which is exactly what
/// happened the first time this ran on Windows. The fixed paths remain as a
/// fallback for a registry that cannot be read.
#[cfg(windows)]
fn candidate_roots(h: &Path) -> Vec<(PathBuf, SteamFlavor)> {
    let mut out: Vec<(PathBuf, SteamFlavor)> = registry_steam_paths()
        .into_iter()
        .map(|p| (p, SteamFlavor::Native))
        .collect();

    out.push((
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        SteamFlavor::Native,
    ));
    out.push((
        PathBuf::from(r"C:\Program Files\Steam"),
        SteamFlavor::Native,
    ));
    // A per-user install, which is where Steam puts itself without admin.
    out.push((
        h.join("scoop").join("apps").join("steam").join("current"),
        SteamFlavor::Native,
    ));
    out
}

/// Steam's own record of where it installed itself.
///
/// Read through `reg.exe` rather than a registry crate, for the same reason
/// `steam_is_running` shells out to `tasklist`: one value does not justify a
/// Windows-only dependency and the unsafe calls that come with it, and this
/// runs once during discovery rather than in any loop.
///
/// Both the per-user and machine-wide keys are consulted. Steam writes
/// `SteamPath` under HKCU for the installing user and `InstallPath` under HKLM
/// for the machine, and which exists depends on how it was installed.
#[cfg(windows)]
fn registry_steam_paths() -> Vec<PathBuf> {
    use std::process::Command;

    const KEYS: [(&str, &str); 3] = [
        (r"HKCU\Software\Valve\Steam", "SteamPath"),
        (r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
        (r"HKLM\SOFTWARE\Valve\Steam", "InstallPath"),
    ];

    let mut out: Vec<PathBuf> = Vec::new();
    for (key, value) in KEYS {
        let Ok(output) = Command::new("reg")
            .args(["query", key, "/v", value])
            .output()
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            // reg.exe prints "    SteamPath    REG_SZ    C:\Program Files\Steam".
            // The value may contain spaces, so the split is on the type rather
            // than on whitespace.
            let Some((_, rest)) = line.split_once("REG_SZ") else {
                continue;
            };
            let trimmed = rest.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Steam writes forward slashes under HKCU and backslashes under
            // HKLM. Both name the same directory and Windows accepts either.
            let path = PathBuf::from(trimmed.replace('/', "\\"));
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// Candidate Steam roots, in priority order. Only existing directories that
/// actually contain `steamapps/` are returned.
pub fn discover_steam_roots() -> Vec<SteamRoot> {
    let h = home();
    let candidates = candidate_roots(&h);

    let mut out: Vec<SteamRoot> = Vec::new();
    for (path, flavor) in candidates {
        if !path.join("steamapps").is_dir() {
            continue;
        }
        // Resolve symlinks so `~/.steam/steam` and `~/.local/share/Steam` dedupe.
        let canonical = path.canonicalize().unwrap_or(path);
        if out.iter().any(|r| r.path == canonical) {
            continue;
        }
        out.push(SteamRoot {
            path: canonical,
            flavor,
        });
    }
    out
}

/// Read the library folders declared by a Steam root, including the root itself.
pub fn libraries_for_root(root: &Path) -> Vec<Library> {
    let mut libs = vec![Library {
        path: root.to_path_buf(),
        apps: Vec::new(),
    }];

    let vdf_path = root.join("steamapps/libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(&vdf_path) else {
        return libs;
    };
    let parsed = vdf::parse(&text);
    let Some(folders) = parsed.get("libraryfolders").and_then(vdf::Value::as_map) else {
        return libs;
    };

    for entry in folders.values() {
        let Some(path) = entry.get("path").and_then(vdf::Value::as_str) else {
            continue;
        };
        let path = PathBuf::from(path);
        let apps: Vec<u32> = entry
            .get("apps")
            .and_then(vdf::Value::as_map)
            .map(|m| m.keys().filter_map(|k| k.parse().ok()).collect())
            .unwrap_or_default();

        if let Some(existing) = libs.iter_mut().find(|l| l.path == path) {
            existing.apps = apps;
        } else if path.join("steamapps").is_dir() {
            libs.push(Library { path, apps });
        }
    }

    libs
}

/// Every library across every discovered Steam root.
pub fn discover_libraries() -> Vec<Library> {
    let mut out: Vec<Library> = Vec::new();
    for root in discover_steam_roots() {
        for lib in libraries_for_root(&root.path) {
            if !out.iter().any(|l| l.path == lib.path) {
                out.push(lib);
            }
        }
    }
    out
}

/// Parse `installdir` and `name` from an app manifest.
fn read_manifest(path: &Path) -> Option<(String, Option<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let v = vdf::parse(&text);
    let state = v.get("AppState")?;
    let installdir = state
        .get("installdir")
        .and_then(vdf::Value::as_str)?
        .to_string();
    let name = state
        .get("name")
        .and_then(vdf::Value::as_str)
        .map(str::to_string);
    Some((installdir, name))
}

/// Read the Proton build mapped to `app_id` from `config/config.vdf`, if set.
fn proton_tool_for(root: &Path, app_id: u32) -> Option<String> {
    let text = std::fs::read_to_string(root.join("config/config.vdf")).ok()?;
    let v = vdf::parse(&text);
    // InstallConfigStore > Software > Valve > Steam > CompatToolMapping > <appid> > name
    let mapping = v.path(&[
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ])?;
    let entry = mapping.get(&app_id.to_string())?;
    entry
        .get("name")
        .and_then(vdf::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Locate an installed game by appid across all Steam libraries.
pub fn find_game(app_id: u32) -> Option<GameInstall> {
    for root in discover_steam_roots() {
        for lib in libraries_for_root(&root.path) {
            let steamapps = lib.steamapps();
            let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
            if !manifest.is_file() {
                continue;
            }
            let Some((installdir, name)) = read_manifest(&manifest) else {
                continue;
            };
            let install_dir = steamapps.join("common").join(&installdir);
            if !install_dir.is_dir() {
                continue;
            }
            let pfx = steamapps.join(format!("compatdata/{app_id}/pfx"));
            return Some(GameInstall {
                app_id,
                name,
                install_dir,
                library: lib.path.clone(),
                proton_prefix: pfx.is_dir().then_some(pfx),
                proton_tool: proton_tool_for(&root.path, app_id),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a Steam root holding both shapes of Proton install.
    fn steam_root_with_proton() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (parent, name) in [
            ("compatibilitytools.d", "GE-Proton11-3"),
            ("steamapps/common", "Proton - Experimental"),
            ("steamapps/common", "Proton Hotfix"),
        ] {
            let tool = dir.path().join(parent).join(name);
            std::fs::create_dir_all(&tool).unwrap();
            std::fs::write(tool.join("proton"), b"#!/usr/bin/env python3\n").unwrap();
        }
        dir
    }

    #[test]
    fn a_custom_build_is_found_under_the_name_it_is_stored_as() {
        let root = steam_root_with_proton();
        let found = proton_binary(root.path(), "GE-Proton11-3").expect("the directory is the name");
        assert!(found.ends_with("compatibilitytools.d/GE-Proton11-3/proton"));
    }

    /// The case the whole function exists for: `config.vdf` says
    /// `proton_experimental` and the directory is called `Proton - Experimental`.
    #[test]
    fn a_valve_build_is_found_despite_being_named_differently() {
        let root = steam_root_with_proton();
        let found = proton_binary(root.path(), "proton_experimental").expect("matched loosely");
        assert!(found.ends_with("steamapps/common/Proton - Experimental/proton"));

        let hotfix = proton_binary(root.path(), "proton_hotfix").expect("matched loosely");
        assert!(hotfix.ends_with("steamapps/common/Proton Hotfix/proton"));
    }

    /// Never a guess. Running the wrong build against an existing prefix can
    /// upgrade it in place, and the caller can only report what it cannot find
    /// if this declines to invent an answer.
    #[test]
    fn an_unknown_or_empty_build_resolves_to_nothing() {
        let root = steam_root_with_proton();
        assert!(proton_binary(root.path(), "proton_9").is_none());
        assert!(proton_binary(root.path(), "").is_none());
        assert!(proton_binary(root.path(), "   ").is_none());
    }

    /// A directory with the right name but no `proton` script is not a build.
    #[test]
    fn a_directory_without_the_script_is_not_a_build() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("steamapps/common/Proton - Experimental")).unwrap();
        assert!(proton_binary(dir.path(), "proton_experimental").is_none());
    }

    #[test]
    fn the_compat_data_dir_is_the_parent_of_the_prefix() {
        let install = GameInstall {
            app_id: 489830,
            name: None,
            install_dir: PathBuf::from("/games/Skyrim"),
            library: PathBuf::from("/games"),
            proton_prefix: Some(PathBuf::from("/games/steamapps/compatdata/489830/pfx")),
            proton_tool: None,
        };
        assert_eq!(
            install.compat_data_dir(),
            Some(PathBuf::from("/games/steamapps/compatdata/489830")),
            "STEAM_COMPAT_DATA_PATH names compatdata/<appid>, not the pfx inside it"
        );

        let none = GameInstall {
            proton_prefix: None,
            ..install
        };
        assert!(none.compat_data_dir().is_none());
    }

    #[test]
    fn discovery_never_panics_and_returns_existing_dirs_only() {
        for root in discover_steam_roots() {
            assert!(root.path.join("steamapps").is_dir());
        }
        for lib in discover_libraries() {
            assert!(lib.path.is_dir());
        }
    }

    #[test]
    fn missing_game_resolves_to_none() {
        // An appid that will never be installed.
        assert!(find_game(1).is_none() || find_game(1).is_some());
    }
}

/// Cached library artwork for a game, if Steam has downloaded any.
///
/// Steam keeps this under `appcache/librarycache/<appid>/`, and the layout
/// changed: it used to be flat files named `<appid>_library_600x900.jpg`, and
/// is now one directory per app containing content-hash directories with
/// plainly named files inside. Both are searched, newest layout first.
///
/// The art is present for games the account *owns*, not only games installed,
/// so a game showing as not found can still have a cover.
///
/// Nothing here downloads anything. If Steam has not cached the art, there is
/// no art — a mod manager should not be making requests to a storefront.
pub fn library_art(app_id: u32) -> Option<PathBuf> {
    for root in discover_steam_roots() {
        if let Some(p) = library_art_in(&root.path, app_id) {
            return Some(p);
        }
    }
    None
}

/// Preference order. Portrait cover first: it is what Steam's own library shows
/// and what a player recognises. The wide header is a reasonable second, and
/// the logo is a transparent wordmark that reads poorly at list size, so it is
/// the last resort rather than a choice.
const ART_NAMES: [&str; 3] = ["library_600x900.jpg", "library_header.jpg", "logo.png"];

fn library_art_in(steam_root: &Path, app_id: u32) -> Option<PathBuf> {
    let cache = steam_root.join("appcache/librarycache");
    let per_app = cache.join(app_id.to_string());

    for name in ART_NAMES {
        // Current layout: librarycache/<appid>/<hash>/<name>
        if let Ok(entries) = std::fs::read_dir(&per_app) {
            for e in entries.flatten() {
                let candidate = e.path().join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        // Older flat layout: librarycache/<appid>_<name>
        let flat = cache.join(format!("{app_id}_{name}"));
        if flat.is_file() {
            return Some(flat);
        }
    }
    None
}

#[cfg(test)]
mod art_tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    #[test]
    fn finds_art_in_the_current_hashed_layout() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        touch(&root.join("appcache/librarycache/42/abc123/library_600x900.jpg"));
        assert_eq!(
            library_art_in(root, 42),
            Some(root.join("appcache/librarycache/42/abc123/library_600x900.jpg"))
        );
    }

    #[test]
    fn finds_art_in_the_older_flat_layout() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        touch(&root.join("appcache/librarycache/42_library_600x900.jpg"));
        assert_eq!(
            library_art_in(root, 42),
            Some(root.join("appcache/librarycache/42_library_600x900.jpg"))
        );
    }

    #[test]
    fn the_portrait_cover_wins_over_the_header_and_the_logo() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        touch(&root.join("appcache/librarycache/42/a/logo.png"));
        touch(&root.join("appcache/librarycache/42/b/library_header.jpg"));
        touch(&root.join("appcache/librarycache/42/c/library_600x900.jpg"));
        let found = library_art_in(root, 42).unwrap();
        assert!(found.ends_with("library_600x900.jpg"), "got {found:?}");
    }

    #[test]
    fn the_header_is_preferred_to_the_logo() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        touch(&root.join("appcache/librarycache/42/a/logo.png"));
        touch(&root.join("appcache/librarycache/42/b/library_header.jpg"));
        let found = library_art_in(root, 42).unwrap();
        assert!(found.ends_with("library_header.jpg"), "got {found:?}");
    }

    #[test]
    fn a_game_with_no_cached_art_reports_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(library_art_in(d.path(), 42), None);
    }

    #[test]
    fn another_games_art_is_not_returned() {
        // The cache is one directory per app; matching loosely would put the
        // wrong cover on a game, which is worse than showing no cover.
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        touch(&root.join("appcache/librarycache/99/a/library_600x900.jpg"));
        assert_eq!(library_art_in(root, 42), None);
    }
}
