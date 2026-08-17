//! Compression methods inside a zip.
//!
//! A zip entry names its own codec, and mod authors pick it from whatever
//! dropdown their packer offers. Nothing about the file name says which one
//! they chose, so an engine that reads only deflate refuses real mods for a
//! reason no user can see: SkyHUD ships LZMA, and it failed on import with
//! "archive is not a valid zip: unsupported Zip archive: Compression method not
//! supported".
//!
//! The methods that can be written are round-tripped here. LZMA cannot be —
//! nothing in the Rust ecosystem compresses it into a zip — so that one arrives
//! as `fixtures/lzma.zip`, 800 bytes holding exactly the entries [`fixture`]
//! describes, packed by Python's `zipfile` with `ZIP_LZMA`. It is checked in
//! because the alternative is not testing the case that actually broke. Should
//! the two ever drift apart, the assertions below say so rather than passing
//! quietly.

use apoc_modengine::GameRules;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::CompressionMethod;

fn wilds_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["natives".into(), "reframework".into()],
        root_files: Vec::new(),
        accepts_pak: true,
        rewrap: vec![
            ("autorun".into(), "reframework".into()),
            ("STM".into(), "natives".into()),
        ],
        canonical_case: vec!["STM".into()],
        formats: Vec::new(),
        fomod_dest_prefix: String::new(),
        plugin_extensions: Vec::new(),
        manages_plugin_list: false,
        root_folder: None,
        root_patterns: Vec::new(),
    }
}

/// The same two-option Fluffy AIO the checked-in LZMA fixture contains, byte
/// for byte. The mesh is deliberately large and repetitive: a codec that is
/// wired up but decoding wrong tends to produce short or garbage output, and
/// 4 KB of one value makes that visible.
fn fixture() -> Vec<(String, Vec<u8>)> {
    let ini =
        |name: &str| format!("[modinfo]\nname={name}\nversion=1.0\nauthor=Someone\n").into_bytes();
    vec![
        ("Pack/OptionA/modinfo.ini".to_string(), ini("Option A")),
        (
            "Pack/OptionA/natives/STM/art/a.mesh.241111606".to_string(),
            vec![0xAAu8; 4096],
        ),
        ("Pack/OptionB/modinfo.ini".to_string(), ini("Option B")),
        (
            "Pack/OptionB/reframework/autorun/b.lua".to_string(),
            b"-- lua\nreturn {}\n".to_vec(),
        ),
    ]
}

fn write_zip(path: &Path, entries: &[(String, Vec<u8>)], method: CompressionMethod) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(method);
    for (name, bytes) in entries {
        zip.start_file(name.as_str(), opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// Analyze, stage every deployable option, and return the staged tree as
/// `(relative path, bytes)` sorted so two archives can be compared directly.
fn round_trip(archive: &Path) -> Vec<(String, Vec<u8>)> {
    let bundle = apoc_modengine::analyze_archive_with(archive, &wilds_rules()).unwrap();
    let dest = archive.parent().unwrap().join(format!(
        "staged-{}",
        archive.file_stem().unwrap().to_string_lossy()
    ));
    apoc_modengine::stage_bundle(archive, &bundle, &dest).unwrap();

    let mut out = Vec::new();
    collect(&dest, &dest, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(root, &p, out);
        } else {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read(&p).unwrap()));
        }
    }
}

fn lzma_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lzma.zip")
}

/// Assert a staged tree is the fixture's payload, intact.
fn assert_payload(staged: &[(String, Vec<u8>)], what: &str) {
    let mesh = staged
        .iter()
        .find(|(p, _)| p.ends_with("a.mesh.241111606"))
        .unwrap_or_else(|| panic!("{what}: the mesh was not staged"));
    assert_eq!(mesh.1.len(), 4096, "{what}: mesh staged short");
    assert!(
        mesh.1.iter().all(|b| *b == 0xAA),
        "{what}: mesh decoded to the wrong bytes"
    );

    let lua = staged
        .iter()
        .find(|(p, _)| p.ends_with("b.lua"))
        .unwrap_or_else(|| panic!("{what}: the lua script was not staged"));
    assert_eq!(lua.1, b"-- lua\nreturn {}\n", "{what}: lua decoded wrong");
}

#[test]
fn every_writable_method_stages_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = fixture();

    // Stored is the control: no codec at all, so if this disagrees with the
    // others the disagreement is the codec's.
    let control_path = tmp.path().join("stored.zip");
    write_zip(&control_path, &entries, CompressionMethod::Stored);
    let control = round_trip(&control_path);
    assert!(!control.is_empty(), "the control fixture staged nothing");
    assert_payload(&control, "stored");

    // Deflate64, LZMA and the legacy methods are read-only in the zip crate, so
    // they cannot appear here; LZMA is covered by the fixture test below.
    for (name, method) in [
        ("deflate", CompressionMethod::Deflated),
        ("bzip2", CompressionMethod::Bzip2),
        ("zstd", CompressionMethod::Zstd),
        ("xz", CompressionMethod::Xz),
        ("ppmd", CompressionMethod::Ppmd),
    ] {
        let path = tmp.path().join(format!("{name}.zip"));
        write_zip(&path, &entries, method);
        let staged = round_trip(&path);
        assert_payload(&staged, name);
        assert_eq!(
            control, staged,
            "the same mod packed with {name} staged differently"
        );
    }
}

#[test]
fn an_lzma_archive_imports() {
    // The case that shipped broken. Everything below the codec is shared, so
    // reaching a two-option bundle with its payload intact is the whole claim.
    let bundle = apoc_modengine::analyze_archive_with(&lzma_fixture(), &wilds_rules()).unwrap();
    assert_eq!(bundle.option_count(), 2);

    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("lzma.zip");
    std::fs::copy(lzma_fixture(), &archive).unwrap();
    assert_payload(&round_trip(&archive), "lzma");
}
