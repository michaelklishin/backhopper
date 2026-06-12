// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Pure-function tests for `doctor` helpers that do not need a git
//! repository. End-to-end behavior is covered by
//! `tests/integration/end_to_end_doctor_integration_tests.rs`.

use backhopper_core::model::names::{GitRef, ProjectName, TagGlob, TagName};
use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};

use backhopper_cli::commands::doctor::{count_newer_tags, staleness_note};

fn tag(s: &str) -> TagName {
    TagName::new(s).unwrap()
}

fn pin(project: &str, tag_name: &str) -> Pin {
    Pin::new(ProjectName::new(project).unwrap(), tag(tag_name))
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

#[test]
fn literal_pin_behind_the_store_suggests_series_sync_diff() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.16.0"));
    let resolved = pin("cowlib", "2.16.0");
    let newest = tag("2.17.1");
    let note = staleness_note(&spec, Some(&resolved), Some(&newest)).unwrap();
    assert!(note.contains("2.17.1"), "{note}");
    assert!(note.contains("series sync diff"), "{note}");
}

#[test]
fn pattern_pin_excluded_by_glob_names_the_newer_tag() {
    let spec = PinSpec::Pattern {
        project: ProjectName::new("cowlib").unwrap(),
        pattern: TagGlob::new("2.16.*").unwrap(),
        select: PinSelect::Latest,
    };
    let resolved = pin("cowlib", "2.16.0");
    let newest = tag("2.17.1");
    let note = staleness_note(&spec, Some(&resolved), Some(&newest)).unwrap();
    assert!(note.contains("pattern excludes"), "{note}");
    assert!(note.contains("2.17.1"), "{note}");
}

#[test]
fn pin_at_the_newest_tag_gets_no_staleness_note() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.17.1"));
    let resolved = pin("cowlib", "2.17.1");
    let newest = tag("2.17.1");
    assert_eq!(staleness_note(&spec, Some(&resolved), Some(&newest)), None);
}

#[test]
fn self_pin_gets_no_staleness_note() {
    let spec = PinSpec::SelfRef {
        project: ProjectName::new("rabbit").unwrap(),
        git_ref: GitRef::new("v4.1.x").unwrap(),
        repo_dir_path: None,
    };
    let resolved = pin("rabbit", "v4.1.0");
    let newest = tag("v4.2.0");
    assert_eq!(staleness_note(&spec, Some(&resolved), Some(&newest)), None);
}

#[test]
fn unresolved_pin_gets_no_staleness_note() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.16.0"));
    let newest = tag("2.17.1");
    assert_eq!(staleness_note(&spec, None, Some(&newest)), None);
}
