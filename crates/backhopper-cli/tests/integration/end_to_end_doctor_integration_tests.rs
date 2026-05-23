// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `backhopper doctor`. We drive the real binary via
//! `assert_cmd` against a synthetic config and on-disk snapshot store.

use std::path::Path;

use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};

fn write(p: &Path, body: &str) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, body).unwrap();
}

fn write_config(dir: &Path, body: &str) -> std::path::PathBuf {
    let cfg = dir.join("backhopper.toml");
    write(&cfg, body);
    cfg
}

fn write_snapshot(snapshot_dir: &Path, project: &str, tag: &str) {
    let path = snapshot_dir.join(project).join(format!("{tag}.api.txt"));
    let body = format!(
        "# format-version: 1\n# project: {project}\n# tag: {tag}\n# commit: 0000000000000000000000000000000000000000\n# generated-by: test\n# generated-at: 2026-05-23T00:00:00Z\n\n"
    );
    write(&path, &body);
}

const MIN_CONFIG: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name    = "ra"
git_url = "/nonexistent/ra.git"

[[series]]
name = "rabbitmq-4.1"
pins = [
    { project = "ra", tag = "v2.16.13" },
]
"#;

#[test]
fn doctor_reports_missing_pin_with_non_zero_exit() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), MIN_CONFIG);
    std::fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let a = run(["--config-file-path", cfg.to_str().unwrap(), "doctor"]);
    let text = stdout(&a);
    a.code(1);
    assert!(text.contains("MISSING"), "stdout: {text}");
    assert!(text.contains("0/1 pin(s) covered"));
    assert!(text.contains("backhopper snapshots generate"));
}

#[test]
fn doctor_reports_covered_when_snapshot_present() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), MIN_CONFIG);
    let snapshots = tmp.path().join("snapshots");
    std::fs::create_dir_all(&snapshots).unwrap();
    write_snapshot(&snapshots, "ra", "v2.16.13");
    let a = run(["--config-file-path", cfg.to_str().unwrap(), "doctor"]);
    let text = stdout(&a);
    a.success();
    assert!(text.contains("1/1 pin(s) covered"), "stdout: {text}");
}

#[test]
fn doctor_json_envelope_carries_the_full_payload() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), MIN_CONFIG);
    std::fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let a = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "doctor",
    ]);
    let text = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["command"], "doctor");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["data"]["totals"]["missing"], 1);
    assert_eq!(v["data"]["series"][0]["name"], "rabbitmq-4.1");
}

#[test]
fn doctor_series_filter_drops_unmatched_series() {
    let body = r#"
config_version = 1
[defaults]
snapshot_dir = "snapshots"
[[project]]
name    = "ra"
git_url = "/x"
[[series]]
name = "rabbitmq-4.1"
pins = [{ project = "ra", tag = "v2.16.13" }]
[[series]]
name = "rabbitmq-4.0"
pins = [{ project = "ra", tag = "v2.7.0" }]
"#;
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), body);
    std::fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let a = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "doctor",
        "--series",
        "4.1",
    ]);
    let text = stdout(&a);
    assert!(text.contains("rabbitmq-4.1"));
    assert!(!text.contains("rabbitmq-4.0"));
}

#[test]
fn doctor_lists_unpinned_projects() {
    let body = r#"
config_version = 1
[defaults]
snapshot_dir = "snapshots"
[[project]]
name    = "ra"
git_url = "/x"
[[project]]
name    = "khepri"
git_url = "/y"
[[series]]
name = "stable"
pins = [{ project = "ra", tag = "v2.0.0" }]
"#;
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), body);
    std::fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let a = run(["--config-file-path", cfg.to_str().unwrap(), "doctor"]);
    let text = stdout(&a);
    assert!(
        text.contains("not pinned by any series: khepri"),
        "stdout: {text}"
    );
}
