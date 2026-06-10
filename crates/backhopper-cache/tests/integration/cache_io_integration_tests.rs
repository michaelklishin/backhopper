// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use backhopper_cache::{CacheDir, ENTRY_FORMAT_VERSION};

#[derive(Debug, Serialize)]
struct Key<'a> {
    series: &'a str,
    source_branches: Vec<&'a str>,
}

#[derive(Debug, Serialize, PartialEq)]
struct Freshness<'a> {
    target_tip: &'a str,
    crate_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Payload {
    walked: u32,
    names: Vec<String>,
}

fn sample_key() -> Key<'static> {
    Key {
        series: "rabbitmq-4.0",
        source_branches: vec!["main"],
    }
}

fn sample_freshness() -> Freshness<'static> {
    Freshness {
        target_tip: "aaaa",
        crate_version: "0.13.0",
    }
}

fn sample_payload() -> Payload {
    Payload {
        walked: 487,
        names: vec!["a".into(), "b".into()],
    }
}

#[test]
fn store_then_lookup_round_trips() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let hit: Option<Payload> = cache.lookup(&sample_key(), &sample_freshness()).unwrap();
    assert_eq!(hit, Some(sample_payload()));
}

#[test]
fn lookup_on_an_empty_directory_is_a_miss() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path().join("never-created"));
    let hit: Option<Payload> = cache.lookup(&sample_key(), &sample_freshness()).unwrap();
    assert_eq!(hit, None);
}

#[test]
fn freshness_mismatch_is_a_miss() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let moved_tip = Freshness {
        target_tip: "bbbb",
        crate_version: "0.13.0",
    };
    let hit: Option<Payload> = cache.lookup(&sample_key(), &moved_tip).unwrap();
    assert_eq!(hit, None);
}

#[test]
fn each_freshness_field_invalidates_independently() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let bumped_version = Freshness {
        target_tip: "aaaa",
        crate_version: "0.14.0",
    };
    let hit: Option<Payload> = cache.lookup(&sample_key(), &bumped_version).unwrap();
    assert_eq!(hit, None);
}

#[test]
fn different_keys_live_in_different_entries() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let other_key = Key {
        series: "rabbitmq-4.1",
        source_branches: vec!["main"],
    };
    let hit: Option<Payload> = cache.lookup(&other_key, &sample_freshness()).unwrap();
    assert_eq!(hit, None);
}

#[test]
fn store_overwrites_a_stale_entry_in_place() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    let first = cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let updated = Payload {
        walked: 500,
        names: vec!["c".into()],
    };
    let second = cache
        .store(&sample_key(), &sample_freshness(), &updated)
        .unwrap();
    assert_eq!(first, second);
    let hit: Option<Payload> = cache.lookup(&sample_key(), &sample_freshness()).unwrap();
    assert_eq!(hit, Some(updated));
}

#[test]
fn corrupt_entry_is_a_miss_not_an_error() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    let path = cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    fs::write(&path, b"{ torn").unwrap();
    let hit: Option<Payload> = cache.lookup(&sample_key(), &sample_freshness()).unwrap();
    assert_eq!(hit, None);
}

#[test]
fn entry_file_name_carries_the_format_version_prefix() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    let path = cache.entry_path(&sample_key()).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(&format!("v{ENTRY_FORMAT_VERSION}-")));
    assert!(name.ends_with(".json"));
}

#[test]
fn no_temp_files_remain_after_a_store() {
    let dir = TempDir::new().unwrap();
    let cache = CacheDir::new(dir.path());
    cache
        .store(&sample_key(), &sample_freshness(), &sample_payload())
        .unwrap();
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}
