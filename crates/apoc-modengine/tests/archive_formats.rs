//! Container formats.
//!
//! Nexus ships mods as zip, 7z and rar interchangeably, and which one an author
//! picked says nothing about what is inside. So the same mod, packed three
//! ways, has to analyze and stage identically. These tests build the fixtures
//! rather than checking them in, so they cannot drift out of sync with what the
//! engine claims to support.
//!
//! Rar is the exception: it cannot be created, only read, because the format's
//! compressor is proprietary and no crate can produce one. Its read path is
//! covered by the format-detection unit tests and by staging a real rar during
//! development; what is asserted here is that a rar is at least recognised and
//! reaches the decoder rather than being turned away at the door.

use apoc_modengine::GameRules;
use std::io::Write;
use std::path::Path;

fn wilds_rules() -> GameRules {
    GameRules {
        payload_roots: vec!["natives".into(), "reframework".into()],
        root_files: vec![("dinput8.dll".into(), "dinput8.dll".into())],
        accepts_pak: true,
        rewrap: vec![
            ("autorun".into(), "reframework".into()),
            ("STM".into(), "natives".into()),
        ],
        canonical_case: vec!["STM".into()],
    }
}

/// A two-option Fluffy AIO, the shape most Wilds mods actually ship in.
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
        // A genuinely empty file: it must survive as empty, not vanish.
        (
            "Pack/OptionB/reframework/data/empty.json".to_string(),
            Vec::new(),
        ),
    ]
}

fn write_zip(path: &Path, entries: &[(String, Vec<u8>)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(name.as_str(), opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn write_7z(path: &Path, entries: &[(String, Vec<u8>)]) {
    let mut w = sevenz_rust2::ArchiveWriter::create(path).unwrap();
    for (name, bytes) in entries {
        let entry = sevenz_rust2::ArchiveEntry::new_file(name);
        w.push_archive_entry(entry, Some(std::io::Cursor::new(bytes.clone())))
            .unwrap();
    }
    w.finish().unwrap();
}

/// Analyze, then stage every deployable option, returning the staged tree as
/// `(relative path, bytes)` sorted so two formats can be compared directly.
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

#[test]
fn zip_and_7z_of_the_same_mod_stage_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = fixture();

    let zip_path = tmp.path().join("mod.zip");
    let sz_path = tmp.path().join("mod.7z");
    write_zip(&zip_path, &entries);
    write_7z(&sz_path, &entries);

    let from_zip = round_trip(&zip_path);
    let from_7z = round_trip(&sz_path);

    assert!(!from_zip.is_empty(), "the zip fixture staged nothing");
    assert_eq!(
        from_zip, from_7z,
        "the same mod packed two ways must produce the same staged tree"
    );

    // Contents, not just paths: a decoder that writes empty files would pass a
    // path-only comparison.
    let mesh = from_7z
        .iter()
        .find(|(p, _)| p.ends_with("a.mesh.241111606"))
        .expect("mesh missing");
    assert_eq!(mesh.1.len(), 4096);
    assert!(mesh.1.iter().all(|b| *b == 0xAA));

    let empty = from_7z
        .iter()
        .find(|(p, _)| p.ends_with("empty.json"))
        .expect("empty file was dropped rather than staged");
    assert!(empty.1.is_empty());
}

#[test]
fn a_7z_named_zip_still_opens() {
    // Authors mislabel archives constantly. Format comes from the leading
    // bytes, so the wrong extension must not decide anything.
    let tmp = tempfile::tempdir().unwrap();
    let lying = tmp.path().join("actually-7z.zip");
    write_7z(&lying, &fixture());

    let bundle = apoc_modengine::analyze_archive_with(&lying, &wilds_rules()).unwrap();
    assert_eq!(bundle.option_count(), 2);
}

#[test]
fn a_rar_reaches_the_decoder_rather_than_being_refused() {
    // No crate can create a rar, so this asserts the routing rather than a
    // round trip: a rar header must produce a decoder error about the archive
    // being damaged, never "not a zip, 7z or rar archive".
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("truncated.rar");
    std::fs::write(&path, b"Rar!\x1a\x07\x01\x00\x00\x00\x00\x00").unwrap();

    let err = apoc_modengine::analyze_archive_with(&path, &wilds_rules())
        .unwrap_err()
        .to_string();
    assert!(
        !err.contains("not a zip, 7z or rar"),
        "a rar was refused before reaching the decoder: {err}"
    );
}

#[test]
fn something_that_is_not_an_archive_says_so_plainly() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("readme.txt");
    std::fs::write(&path, b"this is just a text file").unwrap();

    let err = apoc_modengine::analyze_archive_with(&path, &wilds_rules())
        .unwrap_err()
        .to_string();
    assert!(err.contains("readme.txt"), "{err}");
    assert!(err.contains("zip, 7z or rar"), "{err}");
}
