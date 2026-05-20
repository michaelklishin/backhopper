// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_cli::commands::snapshots::LookupAllTagsRow;

#[test]
fn lookup_all_tags_row_serializes_json_keys() {
    let row = LookupAllTagsRow {
        mfa: "seshat:overview/1".into(),
        first_tag: Some("v0.2.0".into()),
        last_tag: Some("v0.6.1".into()),
        tags_present: 6,
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"mfa\":\"seshat:overview/1\""), "{json}");
    assert!(json.contains("\"first_tag\":\"v0.2.0\""), "{json}");
    assert!(json.contains("\"last_tag\":\"v0.6.1\""), "{json}");
    assert!(json.contains("\"tags_present\":6"), "{json}");
}

#[test]
fn lookup_all_tags_row_renders_none_as_null() {
    let row = LookupAllTagsRow {
        mfa: "missing:m/0".into(),
        first_tag: None,
        last_tag: None,
        tags_present: 0,
    };
    let json = serde_json::to_string(&row).unwrap();
    assert!(json.contains("\"first_tag\":null"), "{json}");
    assert!(json.contains("\"last_tag\":null"), "{json}");
    assert!(json.contains("\"tags_present\":0"), "{json}");
}

#[test]
fn lookup_all_tags_row_with_same_first_and_last_tag_indicates_one_tag_only() {
    let row = LookupAllTagsRow {
        mfa: "foo:bar/0".into(),
        first_tag: Some("v1.0.0".into()),
        last_tag: Some("v1.0.0".into()),
        tags_present: 1,
    };
    assert_eq!(row.first_tag, row.last_tag);
    assert_eq!(row.tags_present, 1);
}
