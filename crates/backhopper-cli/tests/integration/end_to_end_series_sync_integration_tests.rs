// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

use backhopper_core::schema::CURRENT_SCHEMA_VERSION;
use backhopper_test_support::{GitRepoFixture, toml_path};

const COMPONENTS_MK: &str = r#"
PROJECT = rabbit
dep_ra = hex 2.16.7
dep_seshat = hex 1.0.1
"#;

fn build_repo() -> (GitRepoFixture, TempDir) {
    let work = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
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
        toml_path(snapshot_dir),
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
    assert_eq!(v["schema_version"], CURRENT_SCHEMA_VERSION);
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

fn run_preview(cfg: &Path, repo_dir: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args: Vec<&str> = vec![
        "--config-file-path",
        cfg.to_str().unwrap(),
        "series",
        "sync",
        "preview",
        "--repo-dir-path",
        repo_dir.to_str().unwrap(),
        "--formatter",
        "text",
    ];
    args.extend_from_slice(extra);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args(&args)
        .assert()
}

#[test]
fn preview_without_series_name_derives_from_branch() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(&cfg, repo.dir.path(), &["--from-branch", "main"]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("name = \"rabbitmq-main\""),
        "derived name: {stdout}"
    );
    assert!(stdout.contains("# inferred from main"), "{stdout}");
}

#[test]
fn preview_with_explicit_series_name_overrides_derivation() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(
        &cfg,
        repo.dir.path(),
        &["--from-branch", "main", "--series-name", "custom-name"],
    );
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("name = \"custom-name\""), "{stdout}");
}

#[test]
fn preview_with_branches_list_emits_one_stanza_per_branch() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(&cfg, repo.dir.path(), &["--branches", "main,main"]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let header_count = stdout.matches("[[series]]").count();
    assert_eq!(header_count, 2, "two stanzas: {stdout}");
}

#[test]
fn preview_with_show_skipped_surfaces_invalid_project_names() {
    let work = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
    repo.write_file(
        "rabbitmq-components.mk",
        "dep_ra = hex 2.16.7\ndep_3bad = hex 1.0.0\n",
    );
    repo.commit("v1");
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(
        &cfg,
        repo.dir.path(),
        &["--from-branch", "main", "--show-skipped"],
    );
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("# skipped 3bad:"), "{stdout}");
}

#[test]
fn preview_without_show_skipped_keeps_skipped_lines_hidden() {
    let work = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
    repo.write_file(
        "rabbitmq-components.mk",
        "dep_ra = hex 2.16.7\ndep_3bad = hex 1.0.0\n",
    );
    repo.commit("v1");
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(&cfg, repo.dir.path(), &["--from-branch", "main"]);
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("# skipped"), "{stdout}");
}

#[test]
fn preview_rejects_series_name_combined_with_multi_branch() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(
        &cfg,
        repo.dir.path(),
        &["--branches", "main", "--series-name", "rabbitmq-x"],
    );
    assert.failure();
}

#[test]
fn preview_requires_a_branch_source() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "series",
            "sync",
            "preview",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
        ])
        .assert();
    assert.failure();
}

#[test]
fn preview_multi_branch_skips_branches_that_lack_components_mk() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(
        &cfg,
        repo.dir.path(),
        &["--branches", "main,nope-does-not-exist", "--show-skipped"],
    );
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        stdout.matches("[[series]]").count(),
        1,
        "only the resolvable branch produces a stanza: {stdout}"
    );
    assert!(
        stderr.contains("warning: skipping branch nope-does-not-exist"),
        "stderr surfaces the skipped branch: {stderr}",
    );
}

#[test]
fn preview_single_branch_failure_is_a_hard_error() {
    let (repo, work) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), &snap, EXISTING_BLOCK);
    let assert = run_preview(
        &cfg,
        repo.dir.path(),
        &["--from-branch", "nope-does-not-exist"],
    );
    assert.failure();
}

#[test]
fn preview_json_envelope_wraps_series_list() {
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
            "preview",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--branches",
            "main",
        ])
        .assert();
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON envelope");
    assert_eq!(v["command"], "series sync preview");
    let data = &v["data"];
    let series = data["series"].as_array().expect("series array");
    assert_eq!(series.len(), 1);
    assert_eq!(series[0]["name"], "rabbitmq-main");
    assert_eq!(series[0]["branch"], "main");
}
