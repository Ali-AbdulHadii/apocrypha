//! Reading a Creation Engine plugin's header.
//!
//! Everything else this engine plans from is a path: a name, a directory, an
//! extension. A plugin order cannot be planned that way, because what a plugin
//! depends on is written inside it. This is the first and only place the engine
//! opens a mod's own file to decide anything.
//!
//! Only the `TES4` record is read, and only its first subrecords. That record
//! is at byte zero of every plugin, is a few hundred bytes, and carries both
//! facts needed: the flags saying what kind of plugin this is, and the `MAST`
//! entries naming its masters. The rest of the file is records of game content
//! and is never touched — this is not an ESP parser and should not become one.
//!
//! Written to be safe on a hostile file rather than complete on a valid one. A
//! plugin arrives inside a mod archive from a mod site, so every length in it is
//! attacker-controlled: each field is bounds-checked against what remains, the
//! subrecord walk is capped, and anything unexpected ends the parse with what
//! was learned so far rather than failing the deploy. A plugin whose header will
//! not parse is not refused — it is simply a plugin whose constraints are
//! unknown, which [`apoc_domain::PluginEntry`] already models.

use apoc_domain::plugins::{kind_from_extension, PluginEntry, PluginKind};
use std::path::Path;

/// The fixed part of a `TES4` record: type, data size, flags, id, revision,
/// version, and one unknown field.
const RECORD_HEADER: usize = 24;

/// Record flag: this plugin is a master.
const FLAG_MASTER: u32 = 0x0000_0001;

/// Record flag: this plugin is light, and takes an index in the extended space.
const FLAG_LIGHT: u32 = 0x0000_0200;

/// Enough of the file to hold the record header and its subrecords.
///
/// A `TES4` record is a few hundred bytes in every plugin anyone ships. Reading
/// a bounded prefix rather than the file means a 4 GB texture archive mistakenly
/// named `.esp` costs one page, not four gigabytes of memory.
pub const HEADER_BYTES: usize = 64 * 1024;

/// The most subrecords worth walking before concluding the file is not a
/// plugin. Real headers have tens; the cap is only there so a crafted length
/// cannot spin.
const MAX_SUBRECORDS: usize = 4096;

/// What a plugin's header says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHeader {
    pub kind: PluginKind,
    /// Masters in the order the header names them, which is the order the game
    /// resolves them in.
    pub masters: Vec<String>,
}

/// Read a plugin header from the start of a plugin file.
///
/// `name` is used only for the extension fallback when the bytes do not say.
/// Returns `None` when this is not a plugin at all, which a caller should treat
/// as "constraints unknown" rather than as an error: the file still deploys.
pub fn parse_header(name: &str, bytes: &[u8]) -> Option<PluginHeader> {
    if bytes.len() < RECORD_HEADER || &bytes[0..4] != b"TES4" {
        return None;
    }

    let data_size = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let flags = u32::from_le_bytes(bytes[8..12].try_into().ok()?);

    let kind = if flags & FLAG_LIGHT != 0 {
        // Checked before the master flag: a light master carries both, and
        // which index space it takes is the distinction that matters.
        PluginKind::LightMaster
    } else if flags & FLAG_MASTER != 0 {
        PluginKind::Master
    } else {
        // A plugin that says nothing is an ordinary one, whatever it is called.
        // The extension is consulted only for a file with no readable flags at
        // all, which this is not.
        let _ = kind_from_extension(name);
        PluginKind::Normal
    };

    // The record claims a length; the file may be shorter, and the claim is not
    // to be trusted over what is actually here.
    let available = bytes.len() - RECORD_HEADER;
    let end = RECORD_HEADER + data_size.min(available);
    let masters = read_masters(&bytes[RECORD_HEADER..end]);

    Some(PluginHeader { kind, masters })
}

/// Walk the `TES4` record's subrecords, collecting every `MAST`.
///
/// A subrecord is a four-byte type, a two-byte length, then that many bytes.
/// `MAST` holds a master's filename as a NUL-terminated string, and each is
/// followed by a `DATA` subrecord that this does not need.
fn read_masters(data: &[u8]) -> Vec<String> {
    let mut masters = Vec::new();
    let mut at = 0usize;

    for _ in 0..MAX_SUBRECORDS {
        // Type plus length, or there is no next subrecord.
        if at + 6 > data.len() {
            break;
        }
        let kind = &data[at..at + 4];
        let Ok(len_bytes) = data[at + 4..at + 6].try_into() else {
            break;
        };
        let len = u16::from_le_bytes(len_bytes) as usize;
        let body = at + 6;
        // A length reaching past the end is a malformed or hostile file. Stop
        // with what was read rather than clamping into the next subrecord and
        // reporting a master nobody declared.
        if body + len > data.len() {
            break;
        }

        if kind == b"MAST" {
            if let Some(name) = read_zstring(&data[body..body + len]) {
                masters.push(name);
            }
        }

        at = body + len;
    }

    masters
}

/// A NUL-terminated string, as the format writes names.
///
/// Not UTF-8: these are Windows-1252 filenames. Only the ASCII range appears in
/// practice, and a byte outside it is replaced rather than dropped, so a name
/// that cannot be read still compares unequal to everything instead of silently
/// becoming another plugin's name.
fn read_zstring(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    if end == 0 {
        return None;
    }
    Some(
        bytes[..end]
            .iter()
            .map(|b| if b.is_ascii() { *b as char } else { '\u{FFFD}' })
            .collect(),
    )
}

/// Turn a deployed plugin file into the list entry describing it.
///
/// `read` supplies the file's leading bytes; a file that cannot be read at all
/// yields an entry with no constraints, exactly like one whose header does not
/// parse. The caller owns the I/O so this stays testable without a filesystem.
pub fn entry_for(game_rel_path: &str, read: impl FnOnce() -> Option<Vec<u8>>) -> PluginEntry {
    let name = game_rel_path
        .rsplit('/')
        .next()
        .unwrap_or(game_rel_path)
        .to_string();

    let Some(bytes) = read() else {
        return PluginEntry::unknown(name);
    };
    match parse_header(&name, &bytes) {
        Some(header) => PluginEntry {
            name,
            enabled: false,
            kind: header.kind,
            masters: header.masters,
            implicit: false,
        },
        None => PluginEntry::unknown(name),
    }
}

/// Read the leading bytes of a plugin from disk.
///
/// Bounded by [`HEADER_BYTES`], because the caller is pointing this at whatever
/// a mod shipped with a plugin extension.
pub fn read_header_bytes(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(HEADER_BYTES as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// The plugins among a set of deployed files, described from what is on disk.
///
/// `game_rel_paths` is what a deployment wrote; `rules` decides which of them
/// carry load order, from the profile's declared extensions. Each surviving file
/// is read where it now lives, because that is the version the game will load —
/// a mod update can change what a plugin depends on, and the staged copy may
/// already be gone.
///
/// Order is preserved and duplicates are dropped, keeping the first: two mods
/// can deploy a file of the same name, and the one that won the conflict is the
/// one on disk.
pub fn deployed_entries(
    game_dir: &Path,
    game_rel_paths: &[String],
    rules: &crate::GameRules,
) -> Vec<PluginEntry> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();

    for path in game_rel_paths {
        if !rules.is_plugin_file(path) {
            continue;
        }
        let entry = entry_for(path, || read_header_bytes(&game_dir.join(path)));
        if seen.iter().any(|n| n.eq_ignore_ascii_case(&entry.name)) {
            continue;
        }
        seen.push(entry.name.clone());
        out.push(entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `TES4` record: the fixed header, then the subrecords given.
    fn plugin_bytes(flags: u32, subrecords: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut data = Vec::new();
        for (kind, body) in subrecords {
            data.extend_from_slice(*kind);
            data.extend_from_slice(&(body.len() as u16).to_le_bytes());
            data.extend_from_slice(body);
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"TES4");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // form id
        out.extend_from_slice(&0u32.to_le_bytes()); // revision
        out.extend_from_slice(&0u16.to_le_bytes()); // version
        out.extend_from_slice(&0u16.to_le_bytes()); // unknown
        out.extend_from_slice(&data);
        out
    }

    fn mast(name: &str) -> (&'static [u8; 4], Vec<u8>) {
        let mut body = name.as_bytes().to_vec();
        body.push(0);
        (b"MAST", body)
    }

    fn data() -> (&'static [u8; 4], Vec<u8>) {
        (b"DATA", vec![0; 8])
    }

    #[test]
    fn masters_are_read_in_the_order_the_header_names_them() {
        let bytes = plugin_bytes(
            0,
            &[
                (b"HEDR", vec![0; 12]),
                mast("Skyrim.esm"),
                data(),
                mast("Dawnguard.esm"),
                data(),
            ],
        );

        let header = parse_header("Mod.esp", &bytes).expect("a plugin");
        assert_eq!(header.kind, PluginKind::Normal);
        assert_eq!(header.masters, vec!["Skyrim.esm", "Dawnguard.esm"]);
    }

    /// The flag, not the extension. Mods ship `.esp` files carrying the master
    /// flag, and the engine loads them in the master block regardless of name.
    #[test]
    fn the_flag_decides_the_kind_even_when_the_extension_disagrees() {
        let master = plugin_bytes(FLAG_MASTER, &[(b"HEDR", vec![0; 12])]);
        assert_eq!(
            parse_header("LooksOrdinary.esp", &master).unwrap().kind,
            PluginKind::Master
        );

        let light = plugin_bytes(FLAG_LIGHT | FLAG_MASTER, &[(b"HEDR", vec![0; 12])]);
        assert_eq!(
            parse_header("AlsoOrdinary.esp", &light).unwrap().kind,
            PluginKind::LightMaster,
            "a light master carries both flags and the index space is what matters"
        );
    }

    #[test]
    fn a_file_that_is_not_a_plugin_is_not_one() {
        assert!(parse_header("x.esp", b"").is_none());
        assert!(parse_header("x.esp", b"short").is_none());
        assert!(parse_header("x.esp", &[0u8; 64]).is_none());
        assert!(
            parse_header("texture.esp", b"DDS |this is not a plugin at all........").is_none(),
            "a mislabelled file is not a plugin"
        );
    }

    /// Every length in this file came from a mod site. None of them is trusted.
    #[test]
    fn lengths_that_reach_past_the_end_stop_the_walk_rather_than_the_process() {
        // A subrecord claiming 60000 bytes inside a record that has a handful.
        let mut bytes = plugin_bytes(0, &[mast("Real.esm"), data()]);
        bytes.extend_from_slice(b"MAST");
        bytes.extend_from_slice(&60000u16.to_le_bytes());
        bytes.extend_from_slice(b"truncated");

        let header = parse_header("Mod.esp", &bytes).expect("a plugin");
        assert_eq!(
            header.masters,
            vec!["Real.esm"],
            "what was read is kept; the impossible claim is dropped"
        );
    }

    #[test]
    fn a_record_claiming_more_data_than_the_file_holds_is_read_to_the_end() {
        let mut bytes = plugin_bytes(0, &[mast("Skyrim.esm"), data()]);
        // Rewrite the record's data size to something far larger than the file.
        bytes[4..8].copy_from_slice(&999_999u32.to_le_bytes());

        let header = parse_header("Mod.esp", &bytes).expect("a plugin");
        assert_eq!(header.masters, vec!["Skyrim.esm"]);
    }

    #[test]
    fn a_zero_length_or_empty_master_name_is_not_a_master() {
        let bytes = plugin_bytes(
            0,
            &[(b"MAST", vec![]), (b"MAST", vec![0]), mast("Real.esm")],
        );

        let header = parse_header("Mod.esp", &bytes).expect("a plugin");
        assert_eq!(header.masters, vec!["Real.esm"]);
    }

    #[test]
    fn an_unreadable_file_yields_a_plugin_with_no_constraints_rather_than_an_error() {
        let entry = entry_for("Data/Mod.esp", || None);
        assert_eq!(entry.name, "Mod.esp");
        assert!(entry.masters.is_empty());
        assert!(!entry.enabled, "a plugin is switched on by a person");
        assert_eq!(entry.kind, PluginKind::Normal, "from the extension");
    }

    #[test]
    fn the_entry_takes_its_name_from_the_file_not_the_path() {
        let bytes = plugin_bytes(FLAG_MASTER, &[mast("Skyrim.esm"), data()]);
        let entry = entry_for("Data/Deep/Mod.esm", move || Some(bytes.clone()));

        assert_eq!(entry.name, "Mod.esm");
        assert_eq!(entry.kind, PluginKind::Master);
        assert_eq!(entry.masters, vec!["Skyrim.esm"]);
    }
}
