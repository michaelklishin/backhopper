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

fn make_multi_tag_repo(tags: usize) -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    for n in 0..tags {
        let exports: Vec<String> = (0..=n).map(|i| format!("f{i}/0")).collect();
        let functions = (0..=n)
            .map(|i| format!("f{i}() -> ok."))
            .collect::<Vec<_>>()
            .join("\n");
        repo.write_file(
            "src/demo.erl",
            &format!(
                "-module(demo).\n-export([{}]).\n{functions}\n",
                exports.join(", ")
            ),
        );
        repo.commit(&format!("release {n}"));
        repo.tag(&format!("v1.{n}.0"));
    }
    repo
}

fn stored_without_timestamps(
    snapshot_dir: &std::path::Path,
) -> std::collections::BTreeMap<String, String> {
    let mut files = std::collections::BTreeMap::new();
    for project in fs::read_dir(snapshot_dir).unwrap() {
        for entry in fs::read_dir(project.unwrap().path()).unwrap() {
            let path = entry.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let kept: Vec<&str> = text
                .lines()
                .filter(|line| !line.starts_with("# generated-at:"))
                .collect();
            files.insert(
                path.file_name().unwrap().to_string_lossy().into_owned(),
                kept.join("\n"),
            );
        }
    }
    files
}

#[test]
fn project_generate_writes_the_same_snapshots_for_any_worker_count() {
    let repo = make_multi_tag_repo(8);
    let mut stores = Vec::new();
    for jobs in ["1", "5"] {
        let workdir = TempDir::new().unwrap();
        let snapshot_dir = workdir.path().join("snapshots");
        let cfg = write_series_config(&workdir, &repo, &snapshot_dir);
        let generated = run([
            "--formatter",
            "json",
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
            "--jobs",
            jobs,
        ]);
        let env: Value = serde_json::from_str(&stdout(&generated)).expect("envelope parses");
        assert_eq!(env["data"][0]["captured"], 8, "{env}");
        assert!(env["data"][0]["failed"].as_array().unwrap().is_empty());
        stores.push((workdir, stored_without_timestamps(&snapshot_dir)));
    }
    assert_eq!(stores[0].1.len(), 8);
    assert_eq!(stores[0].1, stores[1].1);
}

#[test]
fn a_series_that_names_a_pin_twice_builds_it_once() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snapshots");
    let repo = make_demo_repo();
    let cfg = write_series_config(&workdir, &repo, &snapshot_dir);
    let text = fs::read_to_string(&cfg).unwrap();
    fs::write(
        &cfg,
        text.replace(
            "    { project = \"demo\", tag = \"v1.0.0\" },\n",
            "    { project = \"demo\", tag = \"v1.0.0\" },\n    { project = \"demo\", tag = \"v1.0.0\" },\n",
        ),
    )
    .unwrap();
    let generated = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-1",
        "--jobs",
        "4",
    ]);
    let env: Value = serde_json::from_str(&stdout(&generated)).expect("envelope parses");
    let summary = &env["data"]["summary"];
    assert_eq!(summary["discovered"], 1, "{env}");
    assert_eq!(summary["skipped"], 1, "{env}");
    assert_eq!(summary["failed"], 0, "{env}");
}
