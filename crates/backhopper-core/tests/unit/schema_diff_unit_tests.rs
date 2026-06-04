// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::schema_diff::{SchemaDiff, TypeChange, diff};
use serde_json::json;

#[test]
fn empty_when_values_are_identical() {
    let a = json!({"x": 1, "y": "hi"});
    let b = a.clone();
    let d = diff(1, 2, &a, &b);
    assert!(d.added_paths.is_empty());
    assert!(d.removed_paths.is_empty());
    assert!(d.changed_types.is_empty());
    assert_eq!(d.from, 1);
    assert_eq!(d.to, 2);
}

#[test]
fn added_property_surfaces_in_added_paths() {
    let a = json!({"x": 1});
    let b = json!({"x": 1, "y": 2});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.added_paths, vec!["/y".to_string()]);
    assert!(d.removed_paths.is_empty());
    assert!(d.changed_types.is_empty());
}

#[test]
fn removed_property_surfaces_in_removed_paths() {
    let a = json!({"x": 1, "y": 2});
    let b = json!({"x": 1});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.removed_paths, vec!["/y".to_string()]);
    assert!(d.added_paths.is_empty());
}

#[test]
fn type_change_surfaces_in_changed_types() {
    let a = json!({"x": 1});
    let b = json!({"x": "one"});
    let d = diff(1, 2, &a, &b);
    assert_eq!(
        d.changed_types,
        vec![TypeChange {
            path: "/x".to_owned(),
            old_type: "integer".to_owned(),
            new_type: "string".to_owned(),
        }]
    );
}

#[test]
fn nested_object_additions_carry_full_pointer() {
    let a = json!({"outer": {"x": 1}});
    let b = json!({"outer": {"x": 1, "y": 2}});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.added_paths, vec!["/outer/y".to_string()]);
}

#[test]
fn array_growth_lists_added_indices() {
    let a = json!({"items": [1, 2]});
    let b = json!({"items": [1, 2, 3]});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.added_paths, vec!["/items/2".to_string()]);
}

#[test]
fn array_shrinkage_lists_removed_indices() {
    let a = json!({"items": [1, 2, 3]});
    let b = json!({"items": [1, 2]});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.removed_paths, vec!["/items/2".to_string()]);
}

#[test]
fn pointer_escapes_tilde_and_slash_in_keys() {
    let a = json!({"weird~key/here": 1});
    let b = json!({"weird~key/here": "now"});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.changed_types.len(), 1);
    assert_eq!(d.changed_types[0].path, "/weird~0key~1here");
}

#[test]
fn added_paths_are_sorted_for_determinism() {
    let a = json!({});
    let b = json!({"z": 1, "a": 2, "m": 3});
    let d = diff(1, 2, &a, &b);
    assert_eq!(d.added_paths, vec!["/a", "/m", "/z"]);
}

#[test]
fn serialises_with_snake_case_field_names() {
    let d = SchemaDiff {
        from: 1,
        to: 2,
        added_paths: vec!["/x".into()],
        removed_paths: vec![],
        changed_types: vec![],
    };
    let json = serde_json::to_string(&d).expect("serialise");
    assert!(json.contains("\"from\":1"));
    assert!(json.contains("\"added_paths\""));
    assert!(json.contains("\"removed_paths\""));
    assert!(json.contains("\"changed_types\""));
}

#[test]
fn diff_round_trips_through_json() {
    let d = SchemaDiff {
        from: 1,
        to: 4,
        added_paths: vec!["/envelope/properties/data/summary_row".into()],
        removed_paths: vec!["/legacy/field".into()],
        changed_types: vec![TypeChange {
            path: "/envelope/properties/data/paths".into(),
            old_type: "array".into(),
            new_type: "object".into(),
        }],
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: SchemaDiff = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

#[test]
fn deeply_nested_array_index_change_is_reported_at_the_index() {
    let a = json!({"a": {"b": [{"c": 1}, {"c": 2}]}});
    let b = json!({"a": {"b": [{"c": 1}, {"c": "two"}]}});
    let d = diff(1, 2, &a, &b);
    assert_eq!(
        d.changed_types,
        vec![TypeChange {
            path: "/a/b/1/c".to_owned(),
            old_type: "integer".to_owned(),
            new_type: "string".to_owned(),
        }]
    );
}
