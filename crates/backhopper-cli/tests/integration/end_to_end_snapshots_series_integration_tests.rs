// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `snapshots generate --series` and
//! `snapshots verify --coverage`.

use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

fn make_demo_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "src/demo.erl",
        "-module(demo).\n-export([go/0]).\ngo() -> ok.\n",
    );
    repo.commit("seed");
    repo.tag("v1.0.0");
    repo
}

fn write_series_config(
    workdir: &TempDir,
    repo: &GitRepoFixture,
    snapshot_dir: &PathBuf,
) -> PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "demo-1"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
]
"#,
        toml_path(snapshot_dir),
        toml_path(repo.dir.path())
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

#[test]
fn coverage_reports_missing_pin_before_generation() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    let repo = make_demo_repo();
    let cfg = write_series_config(&workdir, &repo, &snapshot_dir);

    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "verify",
        "--coverage",
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    assert_eq!(data["series_checked"], 1);
    assert_eq!(data["pins_checked"], 1);
    assert_eq!(data["covered"], 0);
    let missing = data["missing_pins"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["series"], "demo-1");
    assert_eq!(missing[0]["project"], "demo");
    assert_eq!(missing[0]["tag"], "v1.0.0");
}

#[test]
fn series_generate_then_coverage_reports_full_coverage() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    let repo = make_demo_repo();
    let cfg = write_series_config(&workdir, &repo, &snapshot_dir);

    let generated = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-1",
    ]);
    let gen_env: Value = serde_json::from_str(&stdout(&generated)).expect("envelope parses");
    let summary = &gen_env["data"]["summary"];
    assert_eq!(summary["discovered"], 1);
    assert_eq!(summary["failed"], 0);

    let cov = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "verify",
        "--coverage",
    ]);
    let cov_env: Value = serde_json::from_str(&stdout(&cov)).expect("envelope parses");
    let cov_data = &cov_env["data"];
    assert_eq!(cov_data["covered"], 1);
    let missing = cov_data["missing_pins"].as_array().unwrap();
    assert!(missing.is_empty(), "missing should be empty after generate");
}

#[test]
fn series_generate_is_idempotent() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    let repo = make_demo_repo();
    let cfg = write_series_config(&workdir, &repo, &snapshot_dir);

    let _ = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-1",
    ])
    .success();
    let second = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-1",
    ]);
    let env: Value = serde_json::from_str(&stdout(&second)).expect("envelope parses");
    let summary = &env["data"]["summary"];
    assert_eq!(summary["discovered"], 0);
    assert_eq!(summary["skipped"], 1);
    assert_eq!(summary["failed"], 0);
}

#[test]
fn series_generate_skips_self_pins_with_a_structured_note() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    fs::create_dir_all(&snapshot_dir).unwrap();
    let repo = make_demo_repo();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["src/**/*.erl"]

[[project]]
name = "demo"
git_url = "{}"

[[project]]
name   = "self-repo"
kind   = "self"
layout = "multi_app"
app_roots = ["deps"]

[[series]]
name = "mixed"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
    {{ project = "self-repo", branch = "main" }},
]
"#,
        toml_path(&snapshot_dir),
        toml_path(repo.dir.path()),
    );
    let cfg_path = workdir.path().join("backhopper.toml");
    fs::write(&cfg_path, body).unwrap();

    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "mixed",
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    let self_skipped = data["self_pins_skipped"].as_array().unwrap();
    assert_eq!(self_skipped.len(), 1);
    assert_eq!(self_skipped[0]["project"], "self-repo");
    assert_eq!(data["summary"]["discovered"], 1);
}

#[test]
fn coverage_skips_self_pins_in_the_count() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    fs::create_dir_all(&snapshot_dir).unwrap();
    let repo = make_demo_repo();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["src/**/*.erl"]

[[project]]
name = "demo"
git_url = "{}"

[[project]]
name   = "self-repo"
kind   = "self"
layout = "multi_app"
app_roots = ["deps"]

[[series]]
name = "mixed"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
    {{ project = "self-repo", branch = "main" }},
]
"#,
        toml_path(&snapshot_dir),
        toml_path(repo.dir.path()),
    );
    let cfg_path = workdir.path().join("backhopper.toml");
    fs::write(&cfg_path, body).unwrap();

    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "verify",
        "--coverage",
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    assert_eq!(data["self_pins_skipped"], 1);
    assert_eq!(data["pins_checked"], 2);
}

#[test]
fn series_generate_rejects_unknown_series_name() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    let repo = make_demo_repo();
    let cfg = write_series_config(&workdir, &repo, &snapshot_dir);

    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "ghost",
    ])
    .failure();
    let err = String::from_utf8_lossy(&a.get_output().stderr);
    assert!(
        err.to_lowercase().contains("series") || err.to_lowercase().contains("ghost"),
        "expected error referencing series/ghost, got: {err}"
    );
}
