// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `--target-repo-dir-path` cross-branch
//! backport assessment (doc 014). Builds source + target repos in
//! TempDirs and drives the CLI against them.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use crate::helpers::fixture::FixtureRepo;

fn make_source_repo() -> FixtureRepo {
    let repo = FixtureRepo::new();
    repo.write_file(
        "deps/rabbitmq_federation_common/src/foo.erl",
        "-module(foo).\n-export([go/0]).\ngo() -> ok.\n",
    );
    repo.commit("add foo");
    repo.tag("source-v1");
    repo
}

fn make_target_repo_without_path() -> FixtureRepo {
    let repo = FixtureRepo::new();
    repo.write_file("README.md", "target branch\n");
    repo.commit("seed");
    repo
}

fn make_target_repo_with_translated_path() -> FixtureRepo {
    let repo = FixtureRepo::new();
    repo.write_file(
        "deps/rabbitmq_federation/src/foo.erl",
        "-module(foo).\n-export([go/0]).\ngo() -> ok.\n",
    );
    repo.commit("seed");
    repo
}

fn write_config(workdir: &TempDir, repo: &FixtureRepo, snapshot_dir: &Path) -> PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["deps/**/src/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "demo-series"
pins = [
    {{ project = "demo", tag = "source-v1" }},
]

[[path_translation]]
name = "fed_split"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
"#,
        snapshot_dir.display(),
        repo.dir.path().display(),
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn generate_snapshot(cfg_path: &Path) {
    run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-series",
    ])
    .success();
}

#[test]
fn missing_target_path_with_translation_emits_path_rename() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_with_translated_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    let results = data["results"]["results"].as_array().unwrap();
    let pin = &results[0];
    let verdict = &pin["verdict"];
    assert_eq!(
        verdict["verdict"], "requires_adaptation",
        "expected RequiresAdaptation, got: {verdict}"
    );
    let reasons = verdict["reasons"].as_array().unwrap();
    let has_path_rename = reasons.iter().any(|r| r["kind"] == "path_rename");
    assert!(
        has_path_rename,
        "expected path_rename reason in {reasons:?}"
    );
    let rename = reasons.iter().find(|r| r["kind"] == "path_rename").unwrap();
    assert_eq!(
        rename["target_path"],
        "deps/rabbitmq_federation/src/foo.erl"
    );
    assert_eq!(rename["translation"]["name"], "fed_split");
}

#[test]
fn missing_target_path_without_translation_emits_inapplicable() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_without_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    let pin = &data["results"]["results"][0];
    let verdict = &pin["verdict"];
    assert_eq!(verdict["verdict"], "inapplicable");
    assert_eq!(verdict["reason"]["reason"], "paths_missing_on_target");
    let paths = verdict["reason"]["paths"].as_array().unwrap();
    assert!(!paths.is_empty());
}

#[test]
fn no_target_repo_dir_path_preserves_legacy_verdict() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let results = env["data"]["results"]["results"].as_array().unwrap();
    let verdict = &results[0]["verdict"];
    let reason_set: Vec<&str> = verdict
        .get("reasons")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|r| r["kind"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        !reason_set.contains(&"path_rename"),
        "no --target-repo-dir-path: path_rename should not appear in {reason_set:?}"
    );
}

#[test]
fn missing_translations_file_is_hard_error() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_without_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
    let a = run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--path-translations-file-path",
        "/this/file/does/not/exist.toml",
        &sha,
    ])
    .failure();
    let err = String::from_utf8_lossy(&a.get_output().stderr);
    assert!(
        err.contains("path_translation") || err.contains("not found"),
        "expected error about missing translations file, got: {err}"
    );
}
