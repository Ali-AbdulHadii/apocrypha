//! Opt-in tests that talk to the real Nexus Mods API.
//!
//! Skipped unless `NEXUS_SMOKE_API_KEY` is set, so the ordinary suite stays
//! offline and deterministic. They exist because everything else about the
//! update check runs against a captured fixture: a green suite says the rules
//! are right, not that the request still works. Header names, the quota headers
//! and the endpoint path are only ever exercised here.
//!
//! Run with:
//!
//! ```bash
//! NEXUS_SMOKE_API_KEY=... cargo test -p apoc-nexus --test live_smoke -- --nocapture
//! ```
//!
//! Read-only: it validates the key and reads one public mod's file list. It
//! never requests a download link, so it cannot consume a download or write
//! anything to the account.

use apoc_nexus::{pick_update, NexusClient, UpdateStatus};

/// A nightly-built mod, so its file list is long and its update chain real.
const DOMAIN: &str = "monsterhunterwilds";
const MOD_ID: u64 = 93;
const OLDEST_KNOWN_FILE: u64 = 111;

fn client() -> Option<NexusClient> {
    let key = std::env::var("NEXUS_SMOKE_API_KEY").ok()?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return None;
    }
    Some(NexusClient::new(
        key,
        "Apocrypha",
        env!("CARGO_PKG_VERSION"),
    ))
}

#[test]
fn the_key_is_accepted_and_the_quota_headers_parse() {
    let Some(c) = client() else {
        eprintln!("skipped: NEXUS_SMOKE_API_KEY is not set");
        return;
    };

    let (user, limits) = c.validate().expect("validate should succeed");
    assert!(!user.name.is_empty(), "the account has a name");

    // The whole rate-limit design reads these from the response rather than
    // hardcoding published numbers, so a rename upstream must fail loudly.
    assert!(
        limits.hourly_remaining.is_some() || limits.daily_remaining.is_some(),
        "at least one quota header was present and parsed"
    );
    assert!(!limits.exhausted(), "a fresh key is not already exhausted");
}

#[test]
fn a_real_mod_listing_resolves_to_an_update() {
    let Some(c) = client() else {
        eprintln!("skipped: NEXUS_SMOKE_API_KEY is not set");
        return;
    };

    let (files, limits) = c
        .mod_files(DOMAIN, MOD_ID)
        .expect("the files endpoint should answer");

    assert!(!files.files.is_empty(), "the mod has files");
    assert!(
        !files.file_updates.is_empty(),
        "this mod is nightly-built, so it has a replacement chain"
    );
    assert!(
        limits.hourly_remaining.is_some(),
        "the quota headers came back on this endpoint too"
    );

    // The oldest file on the page must resolve to something newer, and that
    // something must be downloadable — present in the listing we were given.
    match pick_update(&files, OLDEST_KNOWN_FILE) {
        UpdateStatus::Available(newest) => {
            assert!(
                files.files.iter().any(|f| f.file_id == newest.file_id),
                "the offered update is a file the listing actually contains"
            );
            assert_ne!(newest.file_id, OLDEST_KNOWN_FILE);
        }
        // If the author ever deletes the old nightlies this becomes Unknown,
        // which is a legitimate answer rather than a failure.
        UpdateStatus::Unknown => eprintln!("file {OLDEST_KNOWN_FILE} is no longer on the page"),
        UpdateStatus::UpToDate => panic!("the oldest nightly cannot be up to date"),
    }
}
