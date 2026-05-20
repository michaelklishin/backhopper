// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::errors::SnapshotError;
use backhopper_core::model::names::{CommitSha, FieldName, ProjectName, RecordName, TagName};
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::snapshot::{
    HrlFile, RecordDecl, RecordField, Snapshot, SnapshotHeader,
};
use backhopper_core::snapshot::{format, parser};

fn minimal_header() -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("khepri").unwrap(),
        tag: TagName::new("v0.16.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["include".into()],
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    }
}

fn snapshot_with_record_field(type_repr: Option<String>) -> Snapshot<Canonical> {
    let mut hrl = HrlFile::new("include/khepri.hrl");
    hrl.records.push(RecordDecl {
        name: RecordName::new("if_child_list_length").unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("count").unwrap(),
            type_repr,
        }],
    });
    Snapshot::from_extracted(minimal_header(), vec![], vec![hrl]).into_canonical()
}

#[test]
fn multi_line_record_field_type_round_trips() {
    let multi_line = "non_neg_integer() |\n      khepri_condition:comparison_op(non_neg_integer())";
    let snap = snapshot_with_record_field(Some(multi_line.into()));
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn khepri_v0_16_0_continuation_indent_parses() {
    let canonical = "\
# backhopper snapshot
# format-version: 1
# project: khepri
# tag: v0.16.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: include
# generated-by: backhopper 0.4.0
# generated-at: 1970-01-01T00:00:00Z

header include/khepri.hrl
  record if_child_list_length
    field count :: non_neg_integer() |
                      khepri_condition:comparison_op(non_neg_integer())
";
    let snap = parser::parse(canonical).expect("snapshot should re-parse");
    let f = &snap.headers()[0].records[0].fields[0];
    let repr = f.type_repr.as_deref().unwrap();
    assert!(repr.contains('\n'), "{repr:?}");
    assert!(
        repr.starts_with("non_neg_integer() |"),
        "first line preserved: {repr:?}"
    );
    assert!(
        repr.contains("khepri_condition:comparison_op(non_neg_integer())"),
        "continuation preserved: {repr:?}"
    );
}

#[test]
fn continuation_without_prior_field_is_rejected() {
    let bad = "\
# backhopper snapshot
# format-version: 1
# project: khepri
# tag: v0.16.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: include
# generated-by: backhopper 0.4.0
# generated-at: 1970-01-01T00:00:00Z

header include/khepri.hrl
  record if_child_list_length
        oops_no_field_yet
";
    let err = parser::parse(bad).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnexpectedToken { .. }),
        "{err:?}"
    );
}

#[test]
fn continuation_after_field_without_type_is_rejected() {
    let bad = "\
# backhopper snapshot
# format-version: 1
# project: khepri
# tag: v0.16.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: include
# generated-by: backhopper 0.4.0
# generated-at: 1970-01-01T00:00:00Z

header include/khepri.hrl
  record if_child_list_length
    field count
        stray_continuation_for_typeless_field
";
    let err = parser::parse(bad).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnexpectedToken { .. }),
        "{err:?}"
    );
}

#[test]
fn three_line_record_field_type_round_trips() {
    let three_line = "non_neg_integer() |\n      khepri:something() |\n      khepri:other()";
    let snap = snapshot_with_record_field(Some(three_line.into()));
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    let written = &back.headers()[0].records[0].fields[0]
        .type_repr
        .as_deref()
        .unwrap();
    assert_eq!(written.matches('\n').count(), 2);
}

#[test]
fn single_line_record_field_type_unchanged() {
    let snap = snapshot_with_record_field(Some("non_neg_integer()".into()));
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}

#[test]
fn record_field_without_type_round_trips() {
    let snap = snapshot_with_record_field(None);
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert!(back.headers()[0].records[0].fields[0].type_repr.is_none());
}

#[test]
fn multi_field_record_with_one_multiline_round_trips() {
    let mut hrl = HrlFile::new("include/x.hrl");
    hrl.records.push(RecordDecl {
        name: RecordName::new("r").unwrap(),
        fields: vec![
            RecordField {
                name: FieldName::new("a").unwrap(),
                type_repr: Some("integer()".into()),
            },
            RecordField {
                name: FieldName::new("b").unwrap(),
                type_repr: Some("foo:bar() |\n      foo:baz()".into()),
            },
            RecordField {
                name: FieldName::new("c").unwrap(),
                type_repr: Some("term()".into()),
            },
        ],
    });
    let snap = Snapshot::from_extracted(minimal_header(), vec![], vec![hrl]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
    assert_eq!(back.headers()[0].records[0].fields.len(), 3);
}

#[test]
fn four_space_non_field_line_is_not_treated_as_continuation() {
    let bad = "\
# backhopper snapshot
# format-version: 1
# project: khepri
# tag: v0.16.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: include
# generated-by: backhopper 0.4.0
# generated-at: 1970-01-01T00:00:00Z

header include/khepri.hrl
  record r
    field a :: integer()
    garbage_at_field_indent
";
    let err = parser::parse(bad).unwrap_err();
    assert!(
        matches!(err, SnapshotError::UnexpectedToken { .. }),
        "{err:?}"
    );
}

#[test]
fn two_records_with_multiline_fields_round_trip() {
    let mut hrl = HrlFile::new("include/x.hrl");
    hrl.records.push(RecordDecl {
        name: RecordName::new("alpha").unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("x").unwrap(),
            type_repr: Some("a |\n      b".into()),
        }],
    });
    hrl.records.push(RecordDecl {
        name: RecordName::new("beta").unwrap(),
        fields: vec![RecordField {
            name: FieldName::new("y").unwrap(),
            type_repr: Some("c |\n      d |\n      e".into()),
        }],
    });
    let snap = Snapshot::from_extracted(minimal_header(), vec![], vec![hrl]).into_canonical();
    let text = format::to_string(&snap).unwrap();
    let back = parser::parse(&text).unwrap();
    assert_eq!(snap, back);
}
