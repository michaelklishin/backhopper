// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `backhopper schema show` and `backhopper schema diff`.

use crate::helpers::cli::{run_fails, run_succeeds, stderr, stdout};

#[test]
fn show_emits_json_for_v1() {
    let a = run_succeeds(["--formatter", "json", "schema", "show", "1"]);
    let body = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json envelope");
    let data = v.get("data").expect("data key");
    let schema = data.get("schema").expect("schema key");
    assert_eq!(
        schema
            .get("schema_version")
            .and_then(serde_json::Value::as_u64),
        Some(1),
    );
    assert!(schema.get("envelope").is_some());
    assert_eq!(
        v.get("command").and_then(serde_json::Value::as_str),
        Some("schema show"),
    );
}

#[test]
fn show_text_mode_emits_pretty_schema() {
    let a = run_succeeds(["schema", "show", "1"]);
    let body = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&body).expect("text-mode pretty JSON");
    assert!(v.get("envelope").is_some());
}

#[test]
fn show_unknown_version_fails_with_known_list_in_message() {
    let a = run_fails(["schema", "show", "99"]);
    let err = stderr(&a);
    assert!(err.contains("99"), "stderr must mention requested version");
    assert!(
        err.contains("1") && err.contains("2"),
        "stderr must list every embedded version: {err}"
    );
}

#[test]
fn diff_same_version_reports_no_differences() {
    let a = run_succeeds(["--formatter", "json", "schema", "diff", "1", "1"]);
    let body = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    let data = v.get("data").expect("data");
    assert_eq!(
        data.get("from").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(data.get("to").and_then(serde_json::Value::as_u64), Some(1));
    assert!(
        data.get("added_paths")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        data.get("removed_paths")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        data.get("changed_types")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// Golden pin for the 023 slice: v8 adds exactly the siblings doctor
/// payload on top of v7, nothing else moves.
#[test]
fn diff_v7_to_v8_adds_exactly_the_siblings_doctor_payload() {
    let a = run_succeeds(["--formatter", "json", "schema", "diff", "7", "8"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&a)).expect("valid json");
    let data = v.get("data").expect("data");
    let added: Vec<&str> = data["added_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(added, ["/siblings_doctor_payload"]);
    assert!(data["removed_paths"].as_array().unwrap().is_empty());
}

/// Golden pin for the 024 slice: v9 adds exactly the cache verb
/// payloads on top of v8.
#[test]
fn diff_v8_to_v9_adds_exactly_the_cache_payloads() {
    let a = run_succeeds(["--formatter", "json", "schema", "diff", "8", "9"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&a)).expect("valid json");
    let data = v.get("data").expect("data");
    let added: Vec<&str> = data["added_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(
        added,
        [
            "/cache_list_payload",
            "/cache_mutation_payload",
            "/cache_show_payload",
            "/cache_stats_payload",
        ]
    );
    assert!(data["removed_paths"].as_array().unwrap().is_empty());
}

/// Golden pin for the apply axis: v13 adds exactly the apply-forecast
/// types and fields on top of v12, and removes nothing.
#[test]
fn diff_v12_to_v13_is_add_only() {
    let a = run_succeeds(["--formatter", "json", "schema", "diff", "12", "13"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&a)).expect("valid json");
    let data = v.get("data").expect("data");
    let added: Vec<&str> = data["added_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(
        added,
        [
            "/batch_payload/$defs/ApplyConflictKind",
            "/batch_payload/$defs/ApplyForecast",
            "/batch_payload/$defs/BatchResult/properties/apply",
            "/batch_payload/$defs/Diagnostics/properties/indirect_call_checks",
            "/batch_payload/$defs/IndirectCallForm",
            "/batch_payload/$defs/IndirectCallTally",
            "/batch_payload/$defs/PathApplyOutcome",
            "/batch_payload/$defs/Reason/oneOf/42",
            "/batch_payload/$defs/ResolverClass/enum/7",
            "/batch_payload/$defs/UnassessedReason",
            "/envelope/properties/data/$defs/ApplyConflictKind",
            "/envelope/properties/data/$defs/ApplyForecast",
            "/envelope/properties/data/$defs/Diagnostics/properties/indirect_call_checks",
            "/envelope/properties/data/$defs/IndirectCallForm",
            "/envelope/properties/data/$defs/IndirectCallTally",
            "/envelope/properties/data/$defs/PathApplyOutcome",
            "/envelope/properties/data/$defs/Reason/oneOf/42",
            "/envelope/properties/data/$defs/UnassessedReason",
            "/envelope/properties/data/properties/apply",
            "/summary_row/properties/apply_conflicts",
        ]
    );
    assert!(data["removed_paths"].as_array().unwrap().is_empty());
    assert!(data["changed_types"].as_array().unwrap().is_empty());
}

#[test]
fn diff_unknown_version_fails_cleanly() {
    let a = run_fails(["schema", "diff", "1", "99"]);
    let err = stderr(&a);
    assert!(err.contains("99"));
}

#[test]
fn schema_group_advertises_show_and_diff_in_help() {
    let a = run_succeeds(["schema", "--help"]);
    let body = stdout(&a);
    assert!(body.contains("show"), "schema --help must mention show");
    assert!(body.contains("diff"), "schema --help must mention diff");
    assert!(
        body.contains("supported_envelope_versions"),
        "schema --help must mention supported_envelope_versions",
    );
}

#[test]
fn supported_envelope_versions_json_carries_current_and_supported_lists() {
    let a = run_succeeds([
        "--formatter",
        "json",
        "schema",
        "supported_envelope_versions",
    ]);
    let body = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json envelope");
    let data = v.get("data").expect("data key");
    let current = data
        .get("current_envelope_version")
        .and_then(serde_json::Value::as_u64)
        .expect("current_envelope_version is u64");
    let supported: Vec<u64> = data
        .get("supported_envelope_versions")
        .and_then(serde_json::Value::as_array)
        .expect("supported_envelope_versions is array")
        .iter()
        .map(|v| v.as_u64().expect("each version is u64"))
        .collect();
    assert!(supported.contains(&current), "current must be in supported");
    assert_eq!(supported.first().copied(), Some(1));
    assert_eq!(supported.last().copied(), Some(current));
    let backhopper_version = data
        .get("backhopper_version")
        .and_then(serde_json::Value::as_str)
        .expect("backhopper_version is string");
    assert_eq!(backhopper_version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn supported_envelope_versions_text_format_includes_each_field_label() {
    let a = run_succeeds(["schema", "supported_envelope_versions"]);
    let body = stdout(&a);
    assert!(body.contains("current_envelope_version:"), "{body}");
    assert!(body.contains("supported_envelope_versions:"), "{body}");
    assert!(body.contains("backhopper_version:"), "{body}");
}
