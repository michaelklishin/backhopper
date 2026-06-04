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
}
