//! `nxm://` registration on Linux, through the XDG desktop entry system.
//!
//! Three steps, in order:
//!   1. write a `.desktop` entry declaring `x-scheme-handler/nxm`
//!   2. refresh the desktop database so the MIME cache sees it
//!   3. set it as the default handler for the scheme
//!
//! Two details are load bearing, both learned from the Nexus Mods App and
//! Vortex issue trackers:
//!
//! * `Exec=` points at a small wrapper script rather than the binary directly.
//!   Some `xdg-open` fallbacks mishandle paths that need escaping, so a plain
//!   path with no spaces avoids the whole class of bug.
//! * The wrapper clears `LD_LIBRARY_PATH` and `LD_PRELOAD`. The handler is
//!   launched by the browser and inherits its environment, which has been known
//!   to crash the launched application.

use super::Registration;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DESKTOP_ID: &str = "dev.apocrypha.desktop-manager.desktop";
const WRAPPER_NAME: &str = "apocrypha-nxm-handler.sh";

fn applications_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("applications")
}

/// Where the `.desktop` entry lives.
pub fn desktop_file_path() -> PathBuf {
    applications_dir().join(DESKTOP_ID)
}

/// Where the launcher wrapper lives.
pub fn wrapper_path() -> PathBuf {
    applications_dir().join(WRAPPER_NAME)
}

fn desktop_entry(wrapper: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Apocrypha\n\
         GenericName=Mod Manager\n\
         Comment=Mod manager for PC games\n\
         Exec={} %u\n\
         TryExec={}\n\
         Icon=dev.apocrypha.desktop-manager\n\
         Terminal=false\n\
         Categories=Game;Utility;\n\
         MimeType=x-scheme-handler/nxm;\n\
         StartupWMClass=apocrypha\n\
         StartupNotify=true\n\
         NoDisplay=false\n",
        wrapper.display(),
        wrapper.display(),
    )
}

fn wrapper_script(binary: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         # Launched by the browser for nxm:// links. The browser's environment\n\
         # can carry loader variables that crash unrelated binaries, so clear\n\
         # them before starting the app.\n\
         unset LD_LIBRARY_PATH\n\
         unset LD_PRELOAD\n\
         exec \"{}\" \"$@\"\n",
        binary.display()
    )
}

fn query_default_handler() -> Option<String> {
    let out = Command::new("xdg-mime")
        .args(["query", "default", "x-scheme-handler/nxm"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Inspect the current state without changing anything.
pub fn status() -> Registration {
    let desktop_file = desktop_file_path();
    let current_handler = query_default_handler();
    Registration {
        // The file existing is what "installed" has always meant here, and it is
        // a genuine fact: the entry is on disk whether or not the desktop
        // environment has been persuaded to prefer it. `is_default` is the
        // question of who actually gets the link, and it is answered by asking
        // the system rather than by looking at what we wrote.
        installed: desktop_file.is_file(),
        is_default: current_handler.as_deref() == Some(DESKTOP_ID),
        current_handler,
        location: desktop_file.display().to_string(),
        replaced: None,
    }
}

/// Register Apocrypha as the handler for `nxm://`.
pub fn register(binary: &Path) -> io::Result<Registration> {
    let previous = query_default_handler().filter(|h| h != DESKTOP_ID);

    let dir = applications_dir();
    fs::create_dir_all(&dir)?;

    let wrapper = wrapper_path();
    fs::write(&wrapper, wrapper_script(binary))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    }

    fs::write(desktop_file_path(), desktop_entry(&wrapper))?;

    // Best effort: these tools are not present in every environment, and the
    // handler still works for desktops that read the .desktop file directly.
    let _ = Command::new("update-desktop-database").arg(&dir).status();
    let _ = Command::new("xdg-settings")
        .args(["set", "default-url-scheme-handler", "nxm", DESKTOP_ID])
        .status();
    let _ = Command::new("xdg-mime")
        .args(["default", DESKTOP_ID, "x-scheme-handler/nxm"])
        .status();

    Ok(Registration {
        replaced: previous,
        ..status()
    })
}

/// Remove the registration, handing the scheme back to `previous` if there was
/// one.
///
/// Handing it back matters more than it looks. Taking the scheme from another
/// manager and then simply dropping it leaves the user with nothing handling
/// their downloads and no indication of what changed.
pub fn unregister(previous: Option<&str>) -> io::Result<()> {
    let _ = fs::remove_file(desktop_file_path());
    let _ = fs::remove_file(wrapper_path());
    let _ = Command::new("update-desktop-database")
        .arg(applications_dir())
        .status();

    if let Some(desktop_id) = previous {
        let _ = Command::new("xdg-settings")
            .args(["set", "default-url-scheme-handler", "nxm", desktop_id])
            .status();
        let _ = Command::new("xdg-mime")
            .args(["default", desktop_id, "x-scheme-handler/nxm"])
            .status();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_declares_the_scheme_and_takes_an_argument() {
        let entry = desktop_entry(Path::new("/home/u/.local/share/applications/w.sh"));
        assert!(entry.contains("MimeType=x-scheme-handler/nxm;"));
        assert!(
            entry.contains("%u"),
            "without %u the link is never passed to the app"
        );
        assert!(entry.contains("Type=Application"));
    }

    #[test]
    fn wrapper_clears_loader_variables_inherited_from_the_browser() {
        let s = wrapper_script(Path::new("/usr/bin/apocrypha"));
        assert!(s.starts_with("#!/bin/sh"));
        assert!(s.contains("unset LD_LIBRARY_PATH"));
        assert!(s.contains("unset LD_PRELOAD"));
        assert!(s.contains(r#"exec "/usr/bin/apocrypha" "$@""#));
    }

    #[test]
    fn status_never_panics_even_with_no_desktop_tools() {
        let _ = status();
    }
}
