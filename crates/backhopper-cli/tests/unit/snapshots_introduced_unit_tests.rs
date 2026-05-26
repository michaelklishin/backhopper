// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::iter;

use backhopper_cli::commands::snapshots::{
    IntroducedRow, TagSnapshot, TimelineEntry, compute_introduced_rows,
};
use backhopper_core::model::names::{CommitSha, Mfa, TagName};

fn mfa(s: &str) -> Mfa {
    s.parse().unwrap()
}

fn tag(s: &str) -> TagName {
    s.parse().unwrap()
}

fn sha(byte: u8) -> CommitSha {
    let s: String = iter::repeat_n(format!("{byte:02x}"), 20).collect();
    CommitSha::new(s).unwrap()
}

fn walk(rows: &[(&str, u8, &[bool])]) -> Vec<TagSnapshot> {
    rows.iter()
        .map(|(t, b, p)| TagSnapshot {
            tag: tag(t),
            commit: sha(*b),
            presence: p.to_vec(),
        })
        .collect()
}

#[test]
fn symbol_present_in_only_the_latest_tag_has_equal_first_and_last() {
    let walk = walk(&[
        ("v1.0.0", 1, &[false]),
        ("v1.1.0", 2, &[false]),
        ("v2.0.0", 3, &[true]),
    ]);
    let mfas = vec![mfa("m:f/0")];
    let rows = compute_introduced_rows(&walk, &mfas, false);

    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.first_tag, Some(tag("v2.0.0")));
    assert_eq!(row.last_tag, Some(tag("v2.0.0")));
    assert_eq!(row.first_commit, Some(sha(3)));
    assert_eq!(row.last_commit, Some(sha(3)));
    assert_eq!(row.tags_present, 1);
    assert!(row.timeline.is_none());
}

#[test]
fn symbol_present_only_in_oldest_tag_pins_first_and_last_there() {
    let walk = walk(&[
        ("v1.0.0", 10, &[true]),
        ("v1.1.0", 11, &[false]),
        ("v2.0.0", 12, &[false]),
    ]);
    let rows = compute_introduced_rows(&walk, &[mfa("m:f/0")], false);

    assert_eq!(rows[0].first_tag, Some(tag("v1.0.0")));
    assert_eq!(rows[0].last_tag, Some(tag("v1.0.0")));
    assert_eq!(rows[0].tags_present, 1);
}

#[test]
fn symbol_present_in_every_tag_spans_oldest_to_latest() {
    let walk = walk(&[
        ("v0.1.0", 1, &[true]),
        ("v0.2.0", 2, &[true]),
        ("v1.0.0", 3, &[true]),
    ]);
    let rows = compute_introduced_rows(&walk, &[mfa("m:f/0")], false);

    assert_eq!(rows[0].first_tag, Some(tag("v0.1.0")));
    assert_eq!(rows[0].last_tag, Some(tag("v1.0.0")));
    assert_eq!(rows[0].first_commit, Some(sha(1)));
    assert_eq!(rows[0].last_commit, Some(sha(3)));
    assert_eq!(rows[0].tags_present, 3);
}

#[test]
fn symbol_introduced_mid_history_anchors_to_first_present_snapshot() {
    let walk = walk(&[
        ("v1.0.0", 1, &[false]),
        ("v1.1.0", 2, &[false]),
        ("v1.2.0", 3, &[true]),
        ("v2.0.0", 4, &[true]),
    ]);
    let rows = compute_introduced_rows(&walk, &[mfa("ra_machine:apply/4")], false);

    assert_eq!(rows[0].first_tag, Some(tag("v1.2.0")));
    assert_eq!(rows[0].first_commit, Some(sha(3)));
    assert_eq!(rows[0].last_tag, Some(tag("v2.0.0")));
    assert_eq!(rows[0].last_commit, Some(sha(4)));
    assert_eq!(rows[0].tags_present, 2);
}

#[test]
fn symbol_removed_mid_history_has_last_before_removal_and_no_visible_gap() {
    let walk = walk(&[
        ("v1.0.0", 1, &[true]),
        ("v1.1.0", 2, &[true]),
        ("v2.0.0", 3, &[false]),
    ]);
    let rows = compute_introduced_rows(&walk, &[mfa("m:f/0")], false);

    assert_eq!(rows[0].first_tag, Some(tag("v1.0.0")));
    assert_eq!(rows[0].last_tag, Some(tag("v1.1.0")));
    assert_eq!(rows[0].tags_present, 2);
    // without --timeline, the v2.0.0 gap is invisible.
    assert!(rows[0].timeline.is_none());
}

#[test]
fn timeline_flag_enumerates_every_tag_in_version_order() {
    let walk = walk(&[
        ("v1.0.0", 1, &[true]),
        ("v1.1.0", 2, &[false]),
        ("v1.2.0", 3, &[true]),
    ]);
    let rows = compute_introduced_rows(&walk, &[mfa("m:f/0")], true);
    let timeline = rows[0].timeline.as_ref().expect("timeline requested");

    assert_eq!(
        timeline,
        &vec![
            TimelineEntry {
                tag: tag("v1.0.0"),
                commit: sha(1),
                present: true,
            },
            TimelineEntry {
                tag: tag("v1.1.0"),
                commit: sha(2),
                present: false,
            },
            TimelineEntry {
                tag: tag("v1.2.0"),
                commit: sha(3),
                present: true,
            },
        ]
    );
}

#[test]
fn never_present_symbol_yields_none_endpoints_and_zero_count() {
    let walk = walk(&[("v1.0.0", 1, &[false]), ("v2.0.0", 2, &[false])]);
    let rows = compute_introduced_rows(&walk, &[mfa("never:there/0")], false);

    assert_eq!(rows[0].first_tag, None);
    assert_eq!(rows[0].last_tag, None);
    assert_eq!(rows[0].first_commit, None);
    assert_eq!(rows[0].last_commit, None);
    assert_eq!(rows[0].tags_present, 0);
}

#[test]
fn multiple_mfas_get_independent_results_in_the_same_walk() {
    let walk = walk(&[
        ("v1.0.0", 1, &[true, false]),
        ("v2.0.0", 2, &[true, true]),
        ("v3.0.0", 3, &[false, true]),
    ]);
    let mfas = vec![mfa("a:one/0"), mfa("b:two/0")];
    let rows = compute_introduced_rows(&walk, &mfas, false);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].first_tag, Some(tag("v1.0.0")));
    assert_eq!(rows[0].last_tag, Some(tag("v2.0.0")));
    assert_eq!(rows[0].tags_present, 2);
    assert_eq!(rows[1].first_tag, Some(tag("v2.0.0")));
    assert_eq!(rows[1].last_tag, Some(tag("v3.0.0")));
    assert_eq!(rows[1].tags_present, 2);
}

#[test]
fn introduced_row_json_envelope_omits_timeline_when_absent() {
    let row = IntroducedRow {
        mfa: "ra:m/0".into(),
        first_tag: Some(tag("v1.0.0")),
        first_commit: Some(sha(1)),
        last_tag: Some(tag("v2.0.0")),
        last_commit: Some(sha(2)),
        tags_present: 5,
        timeline: None,
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"first_tag\":\"v1.0.0\""), "{json}");
    assert!(json.contains("\"first_commit\":"), "{json}");
    assert!(json.contains("\"last_tag\":\"v2.0.0\""), "{json}");
    assert!(json.contains("\"tags_present\":5"), "{json}");
    assert!(
        !json.contains("\"timeline\""),
        "skip_serializing_if drops the field: {json}"
    );
}

#[test]
fn introduced_row_json_renders_missing_endpoints_as_null() {
    let row = IntroducedRow {
        mfa: "never:there/0".into(),
        first_tag: None,
        first_commit: None,
        last_tag: None,
        last_commit: None,
        tags_present: 0,
        timeline: None,
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"first_tag\":null"), "{json}");
    assert!(json.contains("\"first_commit\":null"), "{json}");
    assert!(json.contains("\"last_tag\":null"), "{json}");
    assert!(json.contains("\"last_commit\":null"), "{json}");
    assert!(json.contains("\"tags_present\":0"), "{json}");
}

#[test]
fn timeline_serializes_as_array_of_entries_with_tag_commit_and_present() {
    let row = IntroducedRow {
        mfa: "m:f/0".into(),
        first_tag: Some(tag("v1.0.0")),
        first_commit: Some(sha(1)),
        last_tag: Some(tag("v1.0.0")),
        last_commit: Some(sha(1)),
        tags_present: 1,
        timeline: Some(vec![TimelineEntry {
            tag: tag("v1.0.0"),
            commit: sha(1),
            present: true,
        }]),
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"timeline\":["), "{json}");
    assert!(json.contains("\"tag\":\"v1.0.0\""), "{json}");
    assert!(json.contains("\"present\":true"), "{json}");
}

#[test]
fn empty_walk_with_one_mfa_yields_one_row_with_none_endpoints() {
    let rows = compute_introduced_rows(&[], &[mfa("m:f/0")], true);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].first_tag.is_none());
    assert!(rows[0].timeline.as_ref().unwrap().is_empty());
}
