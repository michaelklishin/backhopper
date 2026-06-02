// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cmp::Ordering;

use backhopper_core::git::version_cmp;

#[test]
fn newer_compares_less_so_sort_yields_descending() {
    assert_eq!(version_cmp("v3.13.0", "v3.12.0"), Ordering::Less);
    assert_eq!(version_cmp("v3.12.0", "v3.13.0"), Ordering::Greater);
    assert_eq!(version_cmp("v3.12.0", "v3.12.0"), Ordering::Equal);
}

#[test]
fn equal_strings_compare_equal() {
    assert_eq!(version_cmp("v4.0.0", "v4.0.0"), Ordering::Equal);
}

#[test]
fn numeric_ordering_dominates_string_ordering() {
    // "9" lexically > "10", but numerically 10 > 9, so 10 must come first.
    assert_eq!(version_cmp("v1.10.0", "v1.9.0"), Ordering::Less);
}

#[test]
fn missing_v_prefix_is_handled() {
    assert_eq!(version_cmp("3.12.0", "v3.12.0"), Ordering::Equal);
}

#[test]
fn shorter_version_compares_less_than_longer_with_same_prefix() {
    // [3, 12] vs [3, 12, 0]: shorter has fewer numeric components.
    // Vec ordering: [3,12] < [3,12,0], reversed: Greater.
    assert_eq!(version_cmp("v3.12", "v3.12.0"), Ordering::Greater);
}

#[test]
fn sort_with_version_cmp_yields_newest_first() {
    let mut tags = vec!["v3.10.0", "v3.13.5", "v3.12.0", "v3.13.0"];
    tags.sort_by(|a, b| version_cmp(a, b));
    assert_eq!(tags, vec!["v3.13.5", "v3.13.0", "v3.12.0", "v3.10.0"]);
}

#[test]
fn min_by_version_cmp_returns_latest() {
    let tags = ["v3.10.0", "v3.13.0", "v3.12.0"];
    let latest = tags.iter().min_by(|a, b| version_cmp(a, b)).unwrap();
    assert_eq!(*latest, "v3.13.0");
}

#[test]
fn max_by_version_cmp_returns_oldest() {
    let tags = ["v3.10.0", "v3.13.0", "v3.12.0"];
    let oldest = tags.iter().max_by(|a, b| version_cmp(a, b)).unwrap();
    assert_eq!(*oldest, "v3.10.0");
}

#[test]
fn non_numeric_separators_are_skipped() {
    // dashes, underscores, and `+` separate numeric components.
    assert_eq!(version_cmp("v1-2-3", "v1.2.3"), Ordering::Equal);
}
