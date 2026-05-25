// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::FixtureRepo;

const COMPONENTS_MK: &str = r#"
PROJECT = rabbit
dep_ra = hex 2.16.7
dep_seshat = hex 1.0.1
"#;

fn build_repo() -> (FixtureRepo, TempDir) {
    let work = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("rabbitmq-components.mk", COMPONENTS_MK);
    repo.commit("v1");
    (repo, work)
}

fn write_config(workdir: &Path, snapshot_dir: &Path, existing_series: &str) -> PathBuf {
    let body = format!(
        r#"config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[project]]
name    = "seshat"
git_url = "/tmp/seshat.git"

[[project]]
name    = "khepri"
git_url = "/tmp/khepri.git"

{}"#,
        snapshot_dir.display(),
        existing_series,
    );
    let cfg = workdir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}

fn run(cfg: &Path, repo_dir: &Path, subcmd: &str, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args: Vec<&str> = vec![
        "--config-file-path",
        cfg.to_str().unwrap(),
        "series",
        "sync",
        subcmd,
        "--from-branch",
        "main",
        "--repo-dir-path",
        repo_dir.to_str().unwrap(),
        "--series-name",
        "rabbitmq-4.2",
        "--formatter",
        "text",
    ];
    args.extend_from_slice(extra);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(&args)
        .assert()
}

const EXISTING_BLOCK: &str = r#"[[series]]
name = "rabbitmq-4.2"
pins = [
    { project = "ra", tag = "v2.16.0" },
    { project = "khepri", tag = "v0.15.0" },
]
"#;

#[test]
fn sync_preview_prints_stanza_without_touching_config() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let before = std::fs::read_to_string(&cfg).unwrap();
    let assert = run(&cfg, repo.dir.path(), "preview", &[]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("[[series]]"));
    assert!(stdout.contains("v2.16.7"));
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), before);
}

#[test]
fn sync_diff_shows_only_additive_changes_by_default() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let before = std::fs::read_to_string(&cfg).unwrap();
    let assert = run(&cfg, repo.dir.path(), "diff", &[]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("seshat"), "addition shown: {stdout}");
    assert!(
        !stdout.contains("-    { project = \"khepri\""),
        "khepri kept: {stdout}"
    );
    assert!(
        stdout.contains("conflict ra"),
        "ra conflict reported: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        before,
        "diff did not write"
    );
}

#[test]
fn sync_diff_replace_preview_shows_drops() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run(&cfg, repo.dir.path(), "diff", &["--replace"]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("replace"), "labelled as replace: {stdout}");
    assert!(
        stdout.contains("khepri"),
        "replace drops khepri so it appears in diff: {stdout}"
    );
}

#[test]
fn sync_merge_writes_additive_changes_only() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run(&cfg, repo.dir.path(), "merge", &[]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("added: 1"), "report: {stdout}");
    assert!(stdout.contains("conflicts skipped: 1"));
    let after = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        after.contains("v2.16.0"),
        "ra tag kept (conflict skipped): {after}"
    );
    assert!(after.contains("v0.15.0"), "khepri kept: {after}");
    assert!(after.contains("seshat"), "seshat added: {after}");
    assert!(after.contains("v1.0.1"));
    assert!(
        !after.contains("v2.16.7"),
        "conflicting inferred tag not applied: {after}"
    );
}

#[test]
fn sync_merge_overwrite_existing_applies_conflicts() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run(&cfg, repo.dir.path(), "merge", &["--overwrite-existing"]);
    assert.success();
    let after = std::fs::read_to_string(&cfg).unwrap();
    assert!(after.contains("v2.16.7"), "ra updated: {after}");
    assert!(!after.contains("v2.16.0"));
    assert!(
        after.contains("v0.15.0"),
        "khepri preserved (not in inferred)"
    );
}

#[test]
fn sync_replace_clobbers_block() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run(&cfg, repo.dir.path(), "replace", &[]);
    assert.success();
    let after = std::fs::read_to_string(&cfg).unwrap();
    assert!(after.contains("v2.16.7"));
    assert!(after.contains("seshat"));
    let series_block = after.split("[[series]]").nth(1).unwrap_or("");
    assert!(
        !series_block.contains("khepri"),
        "replace drops the khepri pin: {series_block}"
    );
    assert!(!after.contains("v0.15.0"));
}

#[test]
fn sync_merge_json_envelope_carries_outcome_counts() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "series",
            "sync",
            "merge",
            "--from-branch",
            "main",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--series-name",
            "rabbitmq-4.2",
        ])
        .assert();
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON envelope");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["command"], "series sync merge");
    assert_eq!(v["exit_code"], 0);
    let data = &v["data"];
    assert_eq!(data["series"], "rabbitmq-4.2");
    assert_eq!(data["outcome"]["added"].as_array().unwrap().len(), 1);
    assert_eq!(
        data["outcome"]["skipped_conflicts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(data["dropped_unconfigured"].is_array());
}

#[test]
fn sync_diff_with_overwrite_existing_previews_conflict_application() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let before = std::fs::read_to_string(&cfg).unwrap();
    let assert = run(&cfg, repo.dir.path(), "diff", &["--overwrite-existing"]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("v2.16.7"),
        "previewed merge applies the conflicting tag: {stdout}"
    );
    assert!(
        stdout.contains("v2.16.0"),
        "diff shows the existing tag being replaced: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        before,
        "diff did not write"
    );
}

#[test]
fn sync_overwrite_existing_requires_merge() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run(&cfg, repo.dir.path(), "replace", &["--overwrite-existing"]);
    assert.failure();
}
