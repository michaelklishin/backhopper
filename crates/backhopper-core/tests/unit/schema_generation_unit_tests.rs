// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

#![cfg(feature = "schemars")]

use backhopper_core::schema::{
    CURRENT_SCHEMA_VERSION, MIN_EMBEDDED_VERSION, SchemaError, embedded_versions, rendered_schema,
    schema_value_for,
};

#[test]
fn embedded_versions_includes_current() {
    let v = embedded_versions();
    assert!(v.contains(&CURRENT_SCHEMA_VERSION));
    assert_eq!(v.first().copied(), Some(MIN_EMBEDDED_VERSION));
    assert_eq!(v.last().copied(), Some(CURRENT_SCHEMA_VERSION));
}

#[test]
fn rendered_schema_is_valid_json_for_every_embedded_version() {
    for version in embedded_versions() {
        let s = rendered_schema(version).expect("rendered");
        let _: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
        assert!(s.ends_with('\n'), "trailing newline missing on v{version}");
    }
}

#[test]
fn rendered_schema_is_idempotent() {
    let a = rendered_schema(CURRENT_SCHEMA_VERSION).expect("rendered once");
    let b = rendered_schema(CURRENT_SCHEMA_VERSION).expect("rendered twice");
    assert_eq!(a, b);
}

#[test]
fn schema_value_for_unknown_version_errors_with_known_list() {
    let err = schema_value_for(9999).expect_err("unknown version");
    match err {
        SchemaError::UnknownVersion { requested, known } => {
            assert_eq!(requested, 9999);
            assert_eq!(known, embedded_versions());
        }
    }
}

#[test]
fn schema_value_carries_required_metadata() {
    let v = schema_value_for(1).expect("v1");
    let obj = v.as_object().expect("object");
    assert_eq!(obj.get("schema_version").and_then(|v| v.as_u64()), Some(1));
    assert!(obj.contains_key("title"));
    assert!(obj.contains_key("description"));
    assert!(obj.contains_key("envelope"));
}

#[test]
fn envelope_wrapper_lists_required_fields() {
    let v = schema_value_for(1).expect("v1");
    let envelope = v.get("envelope").expect("envelope key");
    let required = envelope
        .get("required")
        .and_then(|r| r.as_array())
        .expect("required array");
    let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"schema_version"));
    assert!(names.contains(&"command"));
    assert!(names.contains(&"data"));
    assert!(names.contains(&"exit_code"));
}

#[test]
fn v3_carries_summary_row_block() {
    let v = schema_value_for(3).expect("v3");
    assert!(
        v.get("summary_row").is_some(),
        "v3 must surface a summary_row schema"
    );
    assert_eq!(v.get("schema_version").and_then(|x| x.as_u64()), Some(3));
}

#[test]
fn v4_advertises_relative_path_migration_in_description() {
    let v = schema_value_for(4).expect("v4");
    let desc = v
        .get("description")
        .and_then(|x| x.as_str())
        .expect("description string");
    assert!(
        desc.contains("PathRename") || desc.contains("RelativePath"),
        "v4 description must mention the new path surface: {desc}"
    );
}

#[test]
fn every_embedded_version_has_a_schema_version_field_matching_its_index() {
    for v in embedded_versions() {
        let value = schema_value_for(v).expect("rendered");
        assert_eq!(
            value.get("schema_version").and_then(|x| x.as_u64()),
            Some(u64::from(v)),
            "v{v} must declare schema_version == {v}",
        );
    }
}

#[test]
fn v1_frozen_snapshot_remains_byte_stable() {
    let a = rendered_schema(1).expect("v1 render");
    let b = rendered_schema(1).expect("v1 render again");
    assert_eq!(
        a, b,
        "the frozen v1 snapshot must be deterministic across renderings"
    );
}

#[test]
fn v7_embeds_batch_payload_and_v7_summary_row_fields() {
    let v7 = schema_value_for(7).expect("v7 renders");
    let batch = v7.get("batch_payload").expect("v7 carries batch_payload");
    let batch_text = serde_json::to_string(batch).unwrap();
    assert!(
        batch_text.contains("parent_count"),
        "BatchResult.parent_count in v7"
    );
    let row = v7.get("summary_row").expect("v7 carries summary_row");
    let row_text = serde_json::to_string(row).unwrap();
    assert!(row_text.contains("\"series\""), "SummaryRow.series in v7");
    assert!(
        row_text.contains("parent_count"),
        "SummaryRow.parent_count in v7"
    );
}

#[test]
fn frozen_v3_and_v5_predate_the_v7_summary_row_fields() {
    for v in [3u32, 5] {
        let value = schema_value_for(v).expect("frozen render");
        let row = value.get("summary_row").expect("summary_row present");
        let row_text = serde_json::to_string(row).unwrap();
        assert!(
            !row_text.contains("parent_count"),
            "v{v} summary_row must stay frozen without parent_count"
        );
    }
}

#[test]
fn v7_description_names_the_merge_aware_batch_surface() {
    let v7 = schema_value_for(7).expect("v7 renders");
    let desc = v7
        .get("description")
        .and_then(|x| x.as_str())
        .expect("description string");
    assert!(
        desc.contains("parent_count") && desc.contains("merge"),
        "{desc}"
    );
}
