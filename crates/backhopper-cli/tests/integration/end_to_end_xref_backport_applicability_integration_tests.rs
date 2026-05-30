// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `backhopper xref backport-applicability`.
//! Drives the real binary against a synthetic snapshot + reference
//! list and asserts the per-row outcome.

use std::path::Path;

use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};

const SNAPSHOT_WITH_TEST_ONLY: &str = "# backhopper snapshot
# format-version: 3
# project: rabbitmq-server
# tag: v1.0.0
# commit: 0000000000000000000000000000000000000000
# scanned-paths: src
# generated-by: test
# generated-at: 2026-05-29T00:00:00Z

module rabbit_feature_flags
  test_only_export b does_override_nodes/0 export_line=133 body_line=1392
  test_only_export b get_overriden_nodes/0 export_line=133 body_line=1392

module rabbit_khepri
  test_only_export a expand_mnesia_migrations/1 export_line=184
  test_only_export a mnesia_tables_from_migrations/1 export_line=184
";

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn applies_when_reference_set_matches_snapshot() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_khepri.erl"
variant = "a"
functions = ["expand_mnesia_migrations/1", "mnesia_tables_from_migrations/1"]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["applies"], 1);
    assert_eq!(v["data"]["totals"]["applies_with_adaptation"], 0);
    assert_eq!(v["data"]["totals"]["n_a"], 0);
    assert_eq!(v["data"]["rows"][0]["outcome"], "applies");
}

#[test]
fn applies_with_adaptation_when_function_set_drifts() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    // Reference expects an extra function `register_projections/0`
    // that the branch's snapshot does not have.
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_khepri.erl"
variant = "a"
functions = [
    "register_projections/0",
    "expand_mnesia_migrations/1",
    "mnesia_tables_from_migrations/1",
]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["applies_with_adaptation"], 1);
    assert_eq!(v["data"]["rows"][0]["outcome"], "applies-with-adaptation");
    let detail = v["data"]["rows"][0]["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("register_projections/0"),
        "detail was {detail:?}"
    );
}

#[test]
fn n_a_when_module_absent_from_snapshot() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbitmq_jms/src/rabbit_jms_queue_client.erl"
variant = "b"
functions = ["init/1"]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["n_a"], 1);
    assert_eq!(v["data"]["rows"][0]["outcome"], "n_a");
}

#[test]
fn variant_mismatch_yields_applies_with_adaptation() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    // Reference says Variant A for rabbit_feature_flags but the
    // snapshot has them as Variant B.
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_feature_flags.erl"
variant = "a"
functions = ["does_override_nodes/0", "get_overriden_nodes/0"]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["applies_with_adaptation"], 1);
    let detail = v["data"]["rows"][0]["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("variant"),
        "detail should mention variant mismatch, was {detail:?}"
    );
}

#[test]
fn malformed_function_arity_in_reference_yields_n_a_with_parse_error() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_khepri.erl"
variant = "a"
functions = ["this_is_not_arity_shaped"]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["n_a"], 1);
    assert_eq!(v["data"]["rows"][0]["outcome"], "n_a");
    let detail = v["data"]["rows"][0]["detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("expected name/arity"),
        "detail should surface the parse error, was {detail:?}"
    );
}

#[test]
fn non_erl_file_path_yields_n_a() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_khepri.hrl"
variant = "a"
functions = ["foo/0"]
"#,
    );
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["n_a"], 1);
    assert_eq!(v["data"]["rows"][0]["outcome"], "n_a");
    assert_eq!(v["data"]["rows"][0]["module"], serde_json::Value::Null);
}

#[test]
fn empty_reference_list_yields_zero_rows_and_totals() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(&ref_path, "");
    let a = run([
        "--formatter",
        "json",
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("json envelope");
    assert_eq!(v["data"]["totals"]["applies"], 0);
    assert_eq!(v["data"]["totals"]["applies_with_adaptation"], 0);
    assert_eq!(v["data"]["totals"]["n_a"], 0);
    assert_eq!(v["data"]["rows"].as_array().unwrap().len(), 0);
}

#[test]
fn text_format_renders_totals_line() {
    let tmp = TempDir::new().unwrap();
    let snap_path = tmp.path().join("v1.0.0.api.txt");
    write_file(&snap_path, SNAPSHOT_WITH_TEST_ONLY);
    let ref_path = tmp.path().join("reference.toml");
    write_file(
        &ref_path,
        r#"
[[entry]]
file = "deps/rabbit/src/rabbit_khepri.erl"
variant = "a"
functions = ["expand_mnesia_migrations/1", "mnesia_tables_from_migrations/1"]
"#,
    );
    let a = run([
        "xref",
        "backport_applicability",
        "--reference-file-path",
        ref_path.to_str().unwrap(),
        "--snapshot-file-path",
        snap_path.to_str().unwrap(),
    ]);
    let text = stdout(&a);
    assert!(text.contains("applies: 1"), "stdout: {text}");
}
