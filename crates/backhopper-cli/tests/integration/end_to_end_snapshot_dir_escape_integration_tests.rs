// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests: a relative `snapshot_dir` whose
//! resolution walks above the config file's parent must produce a clear,
//! actionable error pointing the user at absolute-path mode.

use tempfile::TempDir;

use crate::helpers::cli::{run, stderr};

#[test]
fn relative_snapshot_dir_above_config_dir_gets_friendly_error() {
    let tmp = TempDir::new().unwrap();
    let cfg = tmp.path().join("backhopper.toml");
    let body = r#"
config_version = 1

[defaults]
snapshot_dir = "../escapes"

[[project]]
name    = "ra"
git_url = "/x"
"#;
    std::fs::write(&cfg, body).unwrap();
    let a = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "config",
        "show",
    ]);
    a.success();
    let a = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "projects",
        "show",
        "--project",
        "ra",
    ])
    .failure();
    let err = stderr(&a);
    assert!(
        err.contains("escapes") || err.contains("snapshot_dir"),
        "stderr should mention snapshot_dir or the escape path, got: {err}"
    );
    assert!(
        err.contains("hint:") && err.contains("absolute"),
        "stderr should carry an absolute-path hint, got: {err}"
    );
}
