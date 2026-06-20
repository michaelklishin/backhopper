// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use tempfile::TempDir;

use backhopper_cache::{
    CacheKeyInputs, ContentOutcome, EvaluationShape, InputOutcome, PinKeyRow, VerdictCache,
    WorkspaceCaches, clear, find_by_key_prefix, prune, scan, stats,
};
use backhopper_core::model::names::CommitSha;
use backhopper_core::model::verdict::{Diagnostics, PatchFacts, SeriesEvaluation, SeriesVerdict};

fn sha(fill: char) -> CommitSha {
    fill.to_string().repeat(40).parse().unwrap()
}

fn key(commit: char) -> CacheKeyInputs {
    CacheKeyInputs {
        shape: EvaluationShape::Check,
        commit: sha(commit),
        series: Some("stable".parse().unwrap()),
        pins: vec![PinKeyRow {
            project: "demo".parse().unwrap(),
            tag: "v1.0.0".parse().unwrap(),
            resolved_tag_sha: Some(sha('d')),
            snapshot_blake3: "snap-hash".to_owned(),
        }],
        source_pins: Vec::new(),
        target: None,
        config_blake3: "config-hash".to_owned(),
        schema_version: 9,
        crate_version: "0.13.0".to_owned(),
    }
}

fn evaluation() -> SeriesEvaluation {
    SeriesEvaluation {
        verdict: SeriesVerdict::from_results(Vec::new()),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
    }
}

/// Walk the full token protocol once: L1 miss, L2 miss, store.
fn seed(cache: &VerdictCache, commit: char, patch: &str) {
    let InputOutcome::MissOnInput(token) = cache.lookup(&key(commit)) else {
        panic!("expected an L1 miss while seeding");
    };
    let content = key(commit).content_key(patch.to_owned(), Some("macro-env".to_owned()));
    let ContentOutcome::Miss(token) = token.lookup_content(&content) else {
        panic!("expected an L2 miss while seeding");
    };
    token.store(&evaluation());
}

#[test]
fn store_then_lookup_hits_on_the_input_level() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    assert!(matches!(cache.lookup(&key('a')), InputOutcome::Hit(_)));
}

#[test]
fn a_new_sha_with_the_same_content_hits_l2_and_mints_an_alias() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");

    // the cascade hop: a fresh SHA, identical content components
    let InputOutcome::MissOnInput(token) = cache.lookup(&key('b')) else {
        panic!("a fresh SHA must miss L1");
    };
    let content = key('b').content_key("patch-hash".to_owned(), Some("macro-env".to_owned()));
    assert!(matches!(
        token.lookup_content(&content),
        ContentOutcome::Hit(_)
    ));
    // the alias minted inside lookup_content serves the fast path now
    assert!(matches!(cache.lookup(&key('b')), InputOutcome::Hit(_)));
}

// The batch path reads the fingerprint off the hit, never re-resolving
// the patch it just skipped: a hit must carry the stored fingerprint,
// equal to the one the content key computes.
#[test]
fn an_l1_hit_returns_the_stored_fingerprint() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let expected = key('a')
        .content_key("patch-hash".to_owned(), Some("macro-env".to_owned()))
        .fingerprint();
    let InputOutcome::Hit(cached) = cache.lookup(&key('a')) else {
        panic!("seeded SHA must hit L1");
    };
    assert!(expected.is_some());
    assert_eq!(cached.fingerprint, expected);
}

// A cascade hop hits L2 with a fresh SHA: the fingerprint is the same
// as the original's, so both rows join one outcome.
#[test]
fn an_l2_hit_returns_the_same_fingerprint_as_the_original() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let original = key('a')
        .content_key("patch-hash".to_owned(), Some("macro-env".to_owned()))
        .fingerprint();
    let InputOutcome::MissOnInput(token) = cache.lookup(&key('b')) else {
        panic!("a fresh SHA must miss L1");
    };
    let content = key('b').content_key("patch-hash".to_owned(), Some("macro-env".to_owned()));
    let ContentOutcome::Hit(cached) = token.lookup_content(&content) else {
        panic!("identical content must hit L2");
    };
    assert_eq!(cached.fingerprint, original);
    // the minted alias carries it onto the L1 fast path too
    let InputOutcome::Hit(aliased) = cache.lookup(&key('b')) else {
        panic!("the alias must serve L1");
    };
    assert_eq!(aliased.fingerprint, original);
}

#[test]
fn drifted_content_misses_l2() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let InputOutcome::MissOnInput(token) = cache.lookup(&key('b')) else {
        panic!("a fresh SHA must miss L1");
    };
    // an adapted pick: same everything except the patch content
    let content = key('b').content_key("other-patch".to_owned(), Some("macro-env".to_owned()));
    assert!(matches!(
        token.lookup_content(&content),
        ContentOutcome::Miss(_)
    ));
}

#[test]
fn every_key_component_flip_misses() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");

    let mut flips: Vec<(&str, CacheKeyInputs)> = Vec::new();
    let mut k = key('a');
    k.commit = sha('b');
    flips.push(("commit", k));
    let mut k = key('a');
    k.pins[0].tag = "v1.1.0".parse().unwrap();
    flips.push(("pin tag", k));
    let mut k = key('a');
    k.pins[0].snapshot_blake3 = "regenerated".to_owned();
    flips.push(("snapshot bytes", k));
    let mut k = key('a');
    k.pins[0].resolved_tag_sha = Some(sha('e'));
    flips.push(("moved tag", k));
    let mut k = key('a');
    k.config_blake3 = "edited-config".to_owned();
    flips.push(("config bytes", k));
    let mut k = key('a');
    k.crate_version = "0.14.0".to_owned();
    flips.push(("crate version", k));
    let mut k = key('a');
    k.schema_version = 10;
    flips.push(("schema version", k));
    let mut k = key('a');
    k.shape = EvaluationShape::Bisect;
    flips.push(("shape", k));
    let mut k = key('a');
    k.series = None;
    flips.push(("series", k));

    for (what, flipped) in flips {
        assert!(
            matches!(cache.lookup(&flipped), InputOutcome::MissOnInput(_)),
            "flipping {what} must miss"
        );
    }
}

fn backdate(path: &Path, age: Duration) {
    let file = fs::File::options().append(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
}

fn backdate_all(dir: &Path, age: Duration) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        backdate(&entry.path(), age);
    }
}

#[test]
fn an_entry_past_the_ttl_reads_as_a_miss_and_is_deleted() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let by_input = dir.path().join(".verdict_cache/by-input");
    backdate_all(&by_input, Duration::from_hours(43 * 24));
    assert!(matches!(
        cache.lookup(&key('a')),
        InputOutcome::MissOnInput(_)
    ));
    assert_eq!(fs::read_dir(&by_input).unwrap().count(), 0);
}

#[test]
fn an_entry_just_inside_the_ttl_still_hits() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    backdate_all(
        &dir.path().join(".verdict_cache/by-input"),
        Duration::from_hours(41 * 24),
    );
    assert!(matches!(cache.lookup(&key('a')), InputOutcome::Hit(_)));
}

#[test]
fn ttl_zero_disables_expiry() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 0);
    seed(&cache, 'a', "patch-hash");
    backdate_all(
        &dir.path().join(".verdict_cache/by-input"),
        Duration::from_hours(365 * 24),
    );
    assert!(matches!(cache.lookup(&key('a')), InputOutcome::Hit(_)));
}

#[test]
fn a_corrupt_entry_reads_as_a_miss_and_is_deleted() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let by_input = dir.path().join(".verdict_cache/by-input");
    for entry in fs::read_dir(&by_input).unwrap().flatten() {
        fs::write(entry.path(), b"{ torn").unwrap();
    }
    assert!(matches!(
        cache.lookup(&key('a')),
        InputOutcome::MissOnInput(_)
    ));
    assert_eq!(fs::read_dir(&by_input).unwrap().count(), 0);
}

#[test]
fn a_foreign_entry_format_version_reads_as_a_miss_and_is_deleted() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let by_input = dir.path().join(".verdict_cache/by-input");
    for entry in fs::read_dir(&by_input).unwrap().flatten() {
        let body = fs::read_to_string(entry.path()).unwrap();
        fs::write(
            entry.path(),
            body.replace(
                "\"entry_format_version\": 1",
                "\"entry_format_version\": 99",
            ),
        )
        .unwrap();
    }
    assert!(matches!(
        cache.lookup(&key('a')),
        InputOutcome::MissOnInput(_)
    ));
    assert_eq!(fs::read_dir(&by_input).unwrap().count(), 0);
}

#[test]
fn store_writes_both_levels_and_stats_sees_them() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let payload = stats(&WorkspaceCaches::at(dir.path()));
    assert_eq!(payload.by_input.entries, 1);
    assert_eq!(payload.by_content.entries, 1);
    assert_eq!(payload.aliases, 0);
    assert!(payload.total_bytes > 0);
    assert!(payload.oldest_entry_at.is_some());
}

#[test]
fn alias_entries_are_counted_and_marked() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let InputOutcome::MissOnInput(token) = cache.lookup(&key('b')) else {
        panic!("fresh SHA must miss");
    };
    let content = key('b').content_key("patch-hash".to_owned(), Some("macro-env".to_owned()));
    assert!(matches!(
        token.lookup_content(&content),
        ContentOutcome::Hit(_)
    ));
    let payload = stats(&WorkspaceCaches::at(dir.path()));
    assert_eq!(payload.by_input.entries, 2);
    assert_eq!(payload.aliases, 1);
    let entries = scan(&WorkspaceCaches::at(dir.path()));
    assert_eq!(entries.iter().filter(|e| e.alias).count(), 1);
}

#[test]
fn prune_and_clear_only_touch_entry_shaped_files() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    let by_input = dir.path().join(".verdict_cache/by-input");
    let keepsake = by_input.join("README.txt");
    fs::write(&keepsake, b"not an entry").unwrap();
    let stale_tmp = by_input.join("v1-aaaa.json.1234.tmp");
    fs::write(&stale_tmp, b"orphan").unwrap();
    backdate(&stale_tmp, Duration::from_hours(2));

    let caches = WorkspaceCaches::at(dir.path());
    let removed = prune(&caches, Duration::from_hours(365 * 24));
    // entries are fresh, so only the orphaned temp file goes
    assert_eq!(removed.entries, 1);
    assert!(keepsake.exists());

    let removed = clear(&caches);
    assert_eq!(removed.entries, 2);
    assert!(keepsake.exists());
    assert_eq!(stats(&caches).total_bytes, 0);
}

#[test]
fn key_prefix_resolution_handles_unique_ambiguous_and_unknown() {
    let dir = TempDir::new().unwrap();
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    seed(&cache, 'b', "other-patch");
    let entries = scan(&WorkspaceCaches::at(dir.path()));
    let first = &entries[0];
    assert_eq!(
        find_by_key_prefix(&entries, &first.key).unwrap().key,
        first.key
    );
    assert!(find_by_key_prefix(&entries, "").is_err());
    assert!(find_by_key_prefix(&entries, "ffffffffffffffff").is_err());
}

#[test]
fn the_daily_sweep_is_marker_gated_and_covers_the_sibling_cache() {
    let dir = TempDir::new().unwrap();
    // a stale sibling-cache entry the sweep should collect
    let siblings = dir.path().join(".siblings_doctor_cache");
    fs::create_dir_all(&siblings).unwrap();
    let stale = siblings.join("v1-deadbeefdeadbeefdeadbeefdeadbeef.json");
    fs::write(&stale, b"{}").unwrap();
    backdate(&stale, Duration::from_hours(60 * 24));

    // the first write of a run sweeps and touches the marker
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'a', "patch-hash");
    assert!(
        !stale.exists(),
        "the sweep must collect expired siblings entries"
    );
    let marker = dir.path().join(".verdict_cache/.last_sweep");
    assert!(marker.exists());

    // a fresh marker suppresses the next sweep
    let stale_again = siblings.join("v1-feedfacefeedfacefeedfacefeedface.json");
    fs::write(&stale_again, b"{}").unwrap();
    backdate(&stale_again, Duration::from_hours(60 * 24));
    let cache = VerdictCache::open(dir.path(), 42);
    seed(&cache, 'b', "other-patch");
    assert!(
        stale_again.exists(),
        "a fresh marker must suppress the sweep"
    );
}
