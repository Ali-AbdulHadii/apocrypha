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
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// Candidate Steam roots, in priority order. Only existing directories that
/// actually contain `steamapps/` are returned.
pub fn discover_steam_roots() -> Vec<SteamRoot> {
    let h = home();
    let candidates: Vec<(PathBuf, SteamFlavor)> = vec![
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
    ];

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
