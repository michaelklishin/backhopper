// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use backhopper_cache::inspect::{KeyLookupError, ScannedEntry, find_by_key_prefix};
use backhopper_cache::verdict::ttl_from_days;
use backhopper_core::model::cache::CacheLevel;

fn entry(key: &str) -> ScannedEntry {
    ScannedEntry {
        key: key.to_owned(),
        level: CacheLevel::ByInput,
        path: PathBuf::from(format!("{key}.json")),
        bytes: 1,
        age: Duration::ZERO,
        created_at: None,
        key_inputs: Value::Null,
        value: Value::Null,
        alias: false,
    }
}

#[test]
fn ttl_from_days_zero_means_no_expiry() {
    assert_eq!(ttl_from_days(0), None);
    assert_eq!(
        ttl_from_days(7),
        Some(Duration::from_secs(7 * 24 * 60 * 60))
    );
}

#[test]
fn unique_prefix_resolves() {
    let entries = [entry("abc123"), entry("def456")];
    let found = find_by_key_prefix(&entries, "abc").unwrap();
    assert_eq!(found.key, "abc123");
}

#[test]
fn unknown_prefix_is_not_found() {
    let entries = [entry("abc123")];
    assert!(matches!(
        find_by_key_prefix(&entries, "zzz"),
        Err(KeyLookupError::NotFound)
    ));
}

// the candidate list is what lets the user extend the prefix
#[test]
fn ambiguous_prefix_names_every_candidate() {
    let entries = [entry("abc123"), entry("abc456")];
    let Err(KeyLookupError::Ambiguous(keys)) = find_by_key_prefix(&entries, "abc") else {
        panic!("expected an ambiguous result");
    };
    assert_eq!(keys, ["abc123", "abc456"]);
}
