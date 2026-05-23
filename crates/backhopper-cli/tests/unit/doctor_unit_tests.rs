// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Pure-function tests for `doctor` helpers that do not need a git
//! repository: only `count_newer_tags` lives here. End-to-end behavior
//! is covered by `tests/integration/end_to_end_doctor_integration_tests.rs`.

use backhopper_core::model::names::TagName;

use backhopper_cli::commands::doctor::count_newer_tags;

fn tag(s: &str) -> TagName {
    TagName::new(s).unwrap()
}

#[test]
fn no_resolved_counts_every_tag_as_ahead() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0"), tag("v3.0.0")];
    assert_eq!(count_newer_tags(&tags, None), 3);
}

#[test]
fn resolved_tag_counts_only_strictly_newer_tags() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0"), tag("v3.0.0")];
    let resolved = tag("v2.0.0");
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 1);
}

#[test]
fn semver_ordering_is_numeric_not_lexicographic() {
    let tags = vec![tag("v2.9.0"), tag("v2.10.0"), tag("v2.11.0"), tag("v3.0.0")];
    let resolved = tag("v2.10.0");
    // Numerically newer than v2.10.0: v2.11.0 and v3.0.0 (2 tags).
    // Lex-newer would only include v3.0.0 and v2.9.0 misordered.
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 2);
}

#[test]
fn resolved_equal_to_newest_yields_zero_ahead() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0")];
    let resolved = tag("v2.0.0");
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 0);
}

#[test]
fn empty_filtered_set_yields_zero() {
    let resolved = tag("v1.0.0");
    assert_eq!(count_newer_tags(&[], Some(&resolved)), 0);
    assert_eq!(count_newer_tags(&[], None), 0);
}
