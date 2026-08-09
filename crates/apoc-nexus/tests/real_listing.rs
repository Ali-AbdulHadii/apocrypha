//! The update picker against a real Nexus response.
//!
//! Every other test for `pick_update` builds its own listing, which proves the
//! rules but not that we read what Nexus actually sends. This one runs against
//! a captured `files.json` — REFramework for Monster Hunter Wilds, chosen
//! because it is nightly-built and therefore has the longest replacement chain
//! of anything in the catalogue.
//!
//! The fixture is the real response with the prose fields removed: changelogs
//! and descriptions are large, contain arbitrary user HTML, and nothing here
//! parses them. Everything the types actually read is untouched, including the
//! keys we ignore, so a field being renamed upstream fails this test rather
//! than silently deserializing to a default.

use apoc_nexus::{pick_update, ModFiles, UpdateStatus};

const FIXTURE: &str = include_str!("fixtures/refr_files.json");

fn listing() -> ModFiles {
    serde_json::from_str(FIXTURE).expect("the captured response still parses")
}

#[test]
fn a_real_response_deserializes_with_its_files_and_update_chain() {
    let l = listing();
    assert_eq!(l.files.len(), 14, "every file survived deserialization");
    assert_eq!(
        l.file_updates.len(),
        13,
        "the replacement chain survived too"
    );

    // If Nexus renamed a field, serde's default would leave these empty or zero
    // and every rule downstream would quietly stop working.
    let first = &l.files[0];
    assert_ne!(first.file_id, 0, "file_id parsed");
    assert!(!first.file_name.is_empty(), "file_name parsed");
    assert!(first.uploaded_timestamp > 0, "uploaded_timestamp parsed");
    assert!(first.category_name.is_some(), "category_name parsed");
}

#[test]
fn the_oldest_file_is_offered_the_newest_one_through_fourteen_hops() {
    // File 111 is the first nightly on the page. Following the author's own
    // replacement links from it should land on the single MAIN file, not on the
    // next nightly along.
    let l = listing();
    match pick_update(&l, 111) {
        UpdateStatus::Available(f) => {
            assert_eq!(f.file_id, 19603);
            assert_eq!(f.category_name.as_deref(), Some("MAIN"));
        }
        other => panic!("expected the newest file, got {other:?}"),
    }
}

#[test]
fn the_current_file_reports_up_to_date() {
    let l = listing();
    assert_eq!(pick_update(&l, 19603), UpdateStatus::UpToDate);
}

#[test]
fn a_file_that_is_not_on_the_page_is_unknown() {
    // Nexus file ids are global rather than per mod, so an id from another mod
    // is a realistic stand-in for a file that has since been deleted.
    let l = listing();
    assert_eq!(pick_update(&l, 1), UpdateStatus::Unknown);
}

#[test]
fn every_intermediate_file_resolves_to_the_same_newest_file() {
    // A user who stopped updating at any point should be offered the current
    // file, not the one that happened to replace theirs.
    let l = listing();
    for f in l.files.iter().filter(|f| f.file_id != 19603) {
        match pick_update(&l, f.file_id) {
            UpdateStatus::Available(newest) => assert_eq!(
                newest.file_id, 19603,
                "file {} should resolve to the newest",
                f.file_id
            ),
            other => panic!("file {} gave {other:?}", f.file_id),
        }
    }
}
