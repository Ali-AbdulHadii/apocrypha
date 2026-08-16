//! `nxm://` registration on Windows, through the per-user registry.
//!
//! Windows has no desktop-entry system; a URL scheme is a key under
//! `Software\Classes` whose `shell\open\command` names the program and how to
//! pass it the link. Vortex and the Nexus Mods app both register the same way,
//! which is why taking the scheme means replacing their value rather than
//! adding beside it — only one program can own it.
//!
//! **`HKCU` only, never `HKLM`.** Per-user needs no elevation, is what the
//! other managers use, and cannot affect anybody else who signs in to the same
//! machine. A mod manager asking for administrator rights to receive download
//! links would be asking for far more than it needs.
//!
//! The value we replace is returned rather than discarded, so it can be put
//! back. This mirrors [`apoc_deploy::loader`], which captures the previous Wine
//! DLL override before writing its own: taking something that belongs to
//! another program and being unable to give it back is the behaviour that makes
//! a manager untrusted.

use super::Registration;
use std::io;
use std::path::Path;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

/// The scheme this registers. A parameter only so the tests can exercise the
/// real registry without ever touching the association a developer is using.
const SCHEME: &str = "nxm";

/// Where the handler command lives, relative to `HKEY_CURRENT_USER`.
fn command_key(scheme: &str) -> String {
    format!(r"Software\Classes\{scheme}\shell\open\command")
}

/// The `shell\open\command` value: the executable, then the link.
///
/// `%1` is quoted because a URL can contain spaces once a browser has finished
/// with it, and the executable is quoted because Windows program paths usually
/// contain one.
pub(super) fn command_for(binary: &Path) -> String {
    format!("\"{}\" \"%1\"", binary.display())
}

/// The executable a `shell\open\command` value would launch.
///
/// Only the first token matters, and it is quoted in every value anybody
/// writes — including Vortex's `"…\Vortex.exe" -d "%1"`, where the arguments
/// after it are none of our business. An unquoted value is read up to the first
/// space, which is what the shell does with it too.
pub(super) fn executable_in(command: &str) -> Option<String> {
    let command = command.trim();
    let exe = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        command.split_whitespace().next()?
    };
    (!exe.is_empty()).then(|| exe.to_string())
}

/// Whether two executable paths are the same program.
///
/// Case-insensitively, because Windows paths are, and a value written by an
/// installer rarely has the same casing as `current_exe`. Separators are
/// normalised for the same reason: both forms reach the shell and both work.
pub(super) fn same_executable(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.replace('/', "\\").to_ascii_lowercase()
    }
    norm(a) == norm(b)
}

fn read_command(scheme: &str) -> Option<String> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(command_key(scheme), KEY_READ)
        .ok()?;
    let value: String = key.get_value("").ok()?;
    (!value.trim().is_empty()).then_some(value)
}

pub(super) fn status_for(scheme: &str) -> Registration {
    let current = read_command(scheme);
    let ours = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string());

    let is_default = match (&current, &ours) {
        (Some(cmd), Some(exe)) => executable_in(cmd)
            .as_deref()
            .is_some_and(|found| same_executable(found, exe)),
        _ => false,
    };

    Registration {
        // On Windows there is no artifact of ours sitting beside the
        // association: either the system launches us for the scheme or it does
        // not. So these two answer the same question, and both are read back
        // from the registry rather than from anything we remember writing.
        // That is the whole bug this replaces — the Linux path reported
        // "installed" because it had written a file, on a platform where the
        // file means nothing.
        installed: is_default,
        is_default,
        current_handler: current,
        location: format!(r"HKCU\{}", command_key(scheme)),
        replaced: None,
    }
}

/// Inspect the current state without changing anything.
pub fn status() -> Registration {
    status_for(SCHEME)
}

pub(super) fn register_for(scheme: &str, binary: &Path) -> io::Result<Registration> {
    let before = status_for(scheme);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // The scheme key itself. `URL Protocol` is what marks a class as a URL
    // handler rather than a file type; without it the shell will not route the
    // link here however correct the command is.
    let (root, _) = hkcu.create_subkey(format!(r"Software\Classes\{scheme}"))?;
    root.set_value("", &format!("URL:{} Protocol", scheme.to_uppercase()))?;
    root.set_value("URL Protocol", &"")?;

    let (cmd, _) = hkcu.create_subkey(command_key(scheme))?;
    cmd.set_value("", &command_for(binary))?;

    Ok(Registration {
        // Only what somebody else owned counts as replaced. Recording our own
        // command would mean a second registration overwrites the memory of
        // what was there before it, and the first Turn off would then restore
        // Apocrypha to Apocrypha.
        replaced: before.current_handler.filter(|_| !before.is_default),
        ..status_for(scheme)
    })
}

/// Register Apocrypha as the handler for `nxm://`.
pub fn register(binary: &Path) -> io::Result<Registration> {
    register_for(SCHEME, binary)
}

pub(super) fn unregister_for(scheme: &str, previous: Option<&str>) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    match previous {
        // Put back what we replaced, rather than deleting the scheme and
        // leaving the user's downloads handled by nothing.
        Some(command) => {
            let (cmd, _) = hkcu.create_subkey(command_key(scheme))?;
            cmd.set_value("", &command)?;
        }
        // Nothing owned it before us, so the state we found is one where the
        // key did not exist.
        None => {
            let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{scheme}"));
        }
    }
    Ok(())
}

/// Remove the registration, handing the scheme back to `previous` if there was
/// one.
pub fn unregister(previous: Option<&str>) -> io::Result<()> {
    unregister_for(SCHEME, previous)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_names_the_binary_and_takes_the_link() {
        let c = command_for(Path::new(r"C:\Program Files\Apocrypha\apocrypha.exe"));
        assert_eq!(c, "\"C:\\Program Files\\Apocrypha\\apocrypha.exe\" \"%1\"");
        assert!(
            c.contains("%1"),
            "without %1 the link is never passed to the app"
        );
    }

    #[test]
    fn the_executable_is_read_back_out_of_a_command() {
        // Our own, and the shape Vortex writes: quoted path, then arguments
        // that are not ours to interpret.
        assert_eq!(
            executable_in(r#""C:\Apps\apocrypha.exe" "%1""#).as_deref(),
            Some(r"C:\Apps\apocrypha.exe")
        );
        assert_eq!(
            executable_in(r#""C:\Program Files\Black Tree Gaming Ltd\Vortex\Vortex.exe" -d "%1""#)
                .as_deref(),
            Some(r"C:\Program Files\Black Tree Gaming Ltd\Vortex\Vortex.exe")
        );
    }

    #[test]
    fn a_path_with_spaces_survives_the_round_trip() {
        // The case an unquoted implementation gets wrong, and every default
        // install path on Windows has a space in it.
        let exe = Path::new(r"C:\Program Files\Apocrypha\apocrypha desktop.exe");
        let back = executable_in(&command_for(exe)).expect("reads back");
        assert_eq!(back, exe.display().to_string());
    }

    #[test]
    fn an_unquoted_command_is_still_read() {
        // Not what this writes, but other programs do, and misreading one would
        // make us claim a handler is ours when it is not.
        assert_eq!(
            executable_in(r"C:\Apps\other.exe %1").as_deref(),
            Some(r"C:\Apps\other.exe")
        );
        assert_eq!(executable_in("").as_deref(), None);
        assert_eq!(executable_in("   ").as_deref(), None);
    }

    #[test]
    fn the_same_program_is_recognised_whatever_the_casing_or_slashes() {
        assert!(same_executable(
            r"C:\Apps\Apocrypha.exe",
            r"c:\apps\apocrypha.exe"
        ));
        assert!(same_executable(
            r"C:/Apps/apocrypha.exe",
            r"C:\Apps\apocrypha.exe"
        ));
        assert!(!same_executable(
            r"C:\Apps\apocrypha.exe",
            r"C:\Apps\vortex.exe"
        ));
    }

    /// The registry half, against the real registry.
    ///
    /// Opt in with `APOC_REGISTRY_TESTS=1`, and even then it never touches the
    /// real `nxm` key: a developer running `cargo test` must not have their
    /// download links quietly taken by their own test suite. The scheme used
    /// here exists for this test and nothing else.
    #[test]
    fn registering_replaces_a_handler_and_turning_off_puts_it_back() {
        if std::env::var_os("APOC_REGISTRY_TESTS").is_none() {
            return;
        }
        const TEST_SCHEME: &str = "apocrypha-test-nxm";
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{TEST_SCHEME}"));

        // Somebody else already owns the scheme.
        let theirs = r#""C:\Program Files\Other\other.exe" -d "%1""#;
        let (cmd, _) = hkcu.create_subkey(command_key(TEST_SCHEME)).unwrap();
        cmd.set_value("", &theirs).unwrap();

        let exe = std::env::current_exe().unwrap();
        let after = register_for(TEST_SCHEME, &exe).unwrap();
        assert!(after.is_default, "the scheme is ours now");
        assert_eq!(
            after.replaced.as_deref(),
            Some(theirs),
            "and what we replaced is remembered"
        );

        unregister_for(TEST_SCHEME, after.replaced.as_deref()).unwrap();
        assert_eq!(
            read_command(TEST_SCHEME).as_deref(),
            Some(theirs),
            "turning off gives it back exactly as it was"
        );

        // And with nothing there beforehand, the key goes away entirely.
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{TEST_SCHEME}"));
        let fresh = register_for(TEST_SCHEME, &exe).unwrap();
        assert!(fresh.replaced.is_none(), "nothing was there to replace");
        unregister_for(TEST_SCHEME, None).unwrap();
        assert!(read_command(TEST_SCHEME).is_none(), "and it is gone");
    }
}
