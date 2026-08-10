//! Links the Windows system library that `unrar_sys` forgets to ask for.
//!
//! The vendored C UnRAR sources call into `advapi32` — the registry
//! (`RegOpenKeyExW`), process tokens (`OpenProcessToken`,
//! `AdjustTokenPrivileges`), SIDs (`AllocateAndInitializeSid`), file security
//! (`SetFileSecurityW`) and the legacy crypto RNG (`CryptGenRandom`) — but
//! `unrar_sys`'s own build script does not emit a link directive for it. On the
//! MSVC target that surfaces as thirteen unresolved externals at the final link,
//! in every binary that pulls this crate in.
//!
//! Emitting it here works because link directives are additive and propagate to
//! whatever is eventually linked: this crate is in the graph of everything that
//! needs UnRAR, so the library reaches the linker even though the crate that
//! actually calls those functions is the one that failed to mention it.
//!
//! It is deliberately not `#[cfg(windows)]` in the source but a check on the
//! *target* OS. A build script runs on the host, and cross-compiling from Linux
//! to Windows would otherwise skip the very case that needs it.
//!
//! This is a workaround for someone else's packaging bug, so it should be
//! removed if `unrar_sys` starts declaring its own dependency — the symptom of
//! it having been fixed upstream is a harmless duplicate `advapi32` on the
//! linker line, not an error.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=advapi32");
    }
}
