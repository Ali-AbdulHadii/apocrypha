//! Registering Apocrypha as the system handler for `nxm://` links.
//!
//! A "Mod manager download" on Nexus opens an `nxm://` URL and lets the
//! operating system decide who receives it. Which program that is, and how it
//! is recorded, is entirely platform business: Linux reads a `.desktop` entry
//! declaring an `x-scheme-handler/nxm` MIME type, Windows reads a key under
//! `Software\Classes`. Neither mechanism resembles the other.
//!
//! So this module is the seam. [`Registration`] is what every caller sees, and
//! the two implementations behind it answer the same three questions in their
//! own terms. Adding macOS means adding a third file, not editing these.
//!
//! **One rule the platform modules share, and the reason this file exists at
//! all:** whether the scheme is ours is answered by asking the system what it
//! will launch, never by checking for something we wrote. The Linux path used
//! to report success on Windows because it had written a `.desktop` file that
//! Windows has no use for, and the interface repeated the claim.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

// Not `cfg(unix)`: this is the fallback as well as the Linux implementation, so
// an unrecognised target gets XDG behaviour rather than no behaviour. That is
// the right guess — the desktop-entry system is what everything other than
// Windows and macOS uses.
#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
use linux as platform;

#[cfg(not(windows))]
pub use linux::{desktop_file_path, wrapper_path, DESKTOP_ID};

use std::io;
use std::path::Path;

/// Current registration state, for showing an honest status in settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// Whether the registration exists at all.
    pub installed: bool,
    /// Whether the system will actually hand `nxm://` links to this program.
    ///
    /// The one the interface should believe. `installed` can be true while this
    /// is false — on Linux the desktop entry is written but the environment
    /// prefers something else — and reporting the first as though it were the
    /// second is how a manager tells someone a thing is set up when it is not.
    pub is_default: bool,
    /// Whichever handler currently owns the scheme, if any. Named so it can be
    /// shown: "Vortex opens these links" is a more useful sentence than "not
    /// set up".
    pub current_handler: Option<String>,
    /// Where the registration lives, for showing in settings and for anyone
    /// reporting a problem. A file path on Linux, a registry key on Windows.
    pub location: String,
    /// What owned the scheme immediately before we took it, if anyone did.
    ///
    /// Set only by [`register`], and deliberately not stored here: this crate
    /// has nowhere durable to keep it. The caller persists it and hands it back
    /// to [`unregister`], the same shape `apoc_deploy::loader` uses for the
    /// Wine DLL override it replaces.
    pub replaced: Option<String>,
}

/// Inspect the current state without changing anything.
pub fn status() -> Registration {
    platform::status()
}

/// Register Apocrypha as the handler for `nxm://`.
///
/// `binary` is the executable to launch. Returns the resulting state so the
/// caller can report exactly what happened rather than assuming success — and
/// callers should, because on both platforms the request can be accepted and
/// still not take effect.
pub fn register(binary: &Path) -> io::Result<Registration> {
    platform::register(binary)
}

/// Remove the registration, handing the scheme back to `previous`.
///
/// `previous` is whatever [`Registration::replaced`] reported when this
/// application took the scheme. Passing it back is what makes taking it
/// reversible; passing `None` removes the registration and leaves the scheme
/// unowned, which is correct only when nothing owned it beforehand.
pub fn unregister(previous: Option<&str>) -> io::Result<()> {
    platform::unregister(previous)
}
