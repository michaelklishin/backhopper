// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::fixture::{FixtureRepo, write_config};

#[test]
fn version_json_payload_has_expected_shape() {
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args(["version", "--formatter", "json"])
        .assert();
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["command"], "version");
    assert_eq!(parsed["data"]["name"], "backhopper");
    assert!(parsed["data"]["version"].is_string());
}

#[test]
fn config_validate_json_returns_ok_true() {
    let work = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "config",
            "validate",
            "--formatter",
            "json",
        ])
        .assert();
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    let parsed: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["data"]["ok"], true);
}