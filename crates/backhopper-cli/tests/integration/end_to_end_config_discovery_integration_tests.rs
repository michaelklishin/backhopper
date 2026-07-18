// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn minimal_config_body() -> &'static str {
    r#"
config_version = 1
[defaults]
fallback_branch = "main"
snapshot_dir    = "snapshots"
"#
}

fn prepare_repo_with_config(dir: &std::path::Path, body: &str) {
    fs::write(dir.join("backhopper.toml"), body).unwrap();
    fs::create_dir_all(dir.join("snapshots")).unwrap();
}

// discovery searches upward from cwd then XDG_CONFIG_HOME or HOME: clear both so the developer's own config cannot leak in
fn cargo_bin() -> Command {
    let mut cmd = Command::cargo_bin("backhopper").unwrap();
    cmd.env_remove("BACKHOPPER_CONFIG_FILE_PATH")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("HOME");
    cmd
}

#[test]
fn config_auto_discovery_finds_backhopper_toml_in_cwd() {
    let dir = TempDir::new().unwrap();
    prepare_repo_with_config(dir.path(), minimal_config_body());
    cargo_bin()
        .current_dir(dir.path())
        .args(["doctor"])
        .assert()
        .success();
}

#[test]
fn config_auto_discovery_walks_up_from_subdirectory() {
    let root = TempDir::new().unwrap();
    prepare_repo_with_config(root.path(), minimal_config_body());
    let nested = root.path().join("a/b/c");
    fs::create_dir_all(&nested).unwrap();
    cargo_bin()
        .current_dir(&nested)
        .args(["doctor"])
        .assert()
        .success();
}

#[test]
fn config_auto_discovery_prefers_dotfile_over_plain() {
    let root = TempDir::new().unwrap();
    // Seed the plain file with invalid TOML so picking it would error.
    fs::write(root.path().join(".backhopper.toml"), minimal_config_body()).unwrap();
    fs::write(
        root.path().join("backhopper.toml"),
        "= this is not valid toml =\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("snapshots")).unwrap();
    cargo_bin()
        .current_dir(root.path())
        .args(["doctor"])
        .assert()
        .success();
}

#[test]
fn config_auto_discovery_stops_at_git_boundary() {
    // outer/backhopper.toml sits above an inner/.git boundary; discovery from subdir must not find it.
    let outer = TempDir::new().unwrap();
    fs::write(outer.path().join("backhopper.toml"), minimal_config_body()).unwrap();
    let inner = outer.path().join("inner");
    fs::create_dir_all(inner.join(".git")).unwrap();
    let subdir = inner.join("subdir");
    fs::create_dir_all(&subdir).unwrap();
    let assert = cargo_bin()
        .current_dir(&subdir)
        .args(["doctor"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.to_lowercase().contains("config")
            || stderr.to_lowercase().contains("backhopper.toml"),
        "expected a 'config not found' style error, got: {stderr}"
    );
}
