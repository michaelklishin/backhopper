// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;
use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{FORMAT_VERSION, Snapshot, SnapshotHeader};
use backhopper_core::snapshot::format;
use backhopper_test_support::toml_path;

fn write_snapshot_with_extractor_version(
    store_root: &Path,
    project: &str,
    tag: &str,
    extractor_version: &str,
) {
    let header = SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src/**/*.erl".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: extractor_version.to_owned(),
        format_version: FORMAT_VERSION,
        dep_pins: Vec::new(),
    };
    let snap = Snapshot::from_extracted(header, vec![], vec![]).into_canonical();
    let dir = store_root.join(project);
    fs::create_dir_all(&dir).unwrap();
    let text = format::to_string(&snap).unwrap();
    fs::write(dir.join(format!("{tag}.api.txt")), text).unwrap();
}

fn write_config(work: &Path, snapshot_dir: &Path, project: &str) -> std::path::PathBuf {
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
[[project]]
name    = "{}"
git_url = "/tmp/does-not-need-to-exist.git"
"#,
        toml_path(snapshot_dir),
        project,
    );
    let cfg = work.join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

#[test]
fn verify_all_flags_stale_extractor_versions() {
    let work = tempdir().unwrap();
    let snap_dir = work.path().join("snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    // One snapshot at version "0", which won't match the running binary.
    write_snapshot_with_extractor_version(&snap_dir, "demo", "v1.0.0", "0");
    let cfg = write_config(work.path(), &snap_dir, "demo");
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "verify",
            "--all",
        ])
        .assert()
        .code(3);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("stale_extractor: 1"),
        "expected stale_extractor: 1 in stdout: {stdout}"
    );
    assert!(
        stdout.contains("snapshots rebuild"),
        "expected remediation hint, got: {stdout}"
    );
}

#[test]
fn verify_all_flags_an_empty_extractor_version_as_unversioned() {
    // pre-versioning snapshots have an empty extractor_version: the
    // most suspect files in the store, not a pass
    let work = tempdir().unwrap();
    let snap_dir = work.path().join("snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    write_snapshot_with_extractor_version(&snap_dir, "demo", "v1.0.0", "");
    let cfg = write_config(work.path(), &snap_dir, "demo");
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "verify",
            "--all",
        ])
        .assert()
        .code(3);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("stale_extractor: 0"),
        "expected stale_extractor: 0, got: {stdout}"
    );
    assert!(
        stdout.contains("unversioned_extractor: 1"),
        "expected unversioned_extractor: 1, got: {stdout}"
    );
    assert!(
        stdout.contains("--refresh-stale"),
        "expected the refresh remedy, got: {stdout}"
    );
}

/// A project backed by a real git repository, so `snapshots generate`
/// has something to build from once it decides to refresh a tag.
fn write_git_project_config(
    work: &Path,
    snapshot_dir: &Path,
    repo_dir: &Path,
    project: &str,
) -> std::path::PathBuf {
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
[[project]]
name    = "{}"
git_url = "{}"
"#,
        toml_path(snapshot_dir),
        project,
        toml_path(repo_dir),
    );
    let cfg = work.join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn init_repo_with_erlang_module_and_tag(repo_dir: &Path, tag: &str) {
    fs::create_dir_all(repo_dir.join("src")).unwrap();
    fs::write(
        repo_dir.join("src/demo.erl"),
        "-module(demo).\n-export([hello/0]).\nhello() -> ok.\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo_dir)
                .status()
                .unwrap()
                .success(),
            "git {args:?} failed"
        );
    };
    run(&["init", "--initial-branch=main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
    run(&["tag", tag]);
}

#[test]
fn generate_refresh_stale_rebuilds_an_unversioned_snapshot() {
    let work = tempdir().unwrap();
    let repo_dir = work.path().join("repo");
    init_repo_with_erlang_module_and_tag(&repo_dir, "v1.0.0");
    let snap_dir = work.path().join("snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    write_snapshot_with_extractor_version(&snap_dir, "demo", "v1.0.0", "");
    let cfg = write_git_project_config(work.path(), &snap_dir, &repo_dir, "demo");

    let doctor_before = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "verify",
            "--all",
        ])
        .assert()
        .code(3);
    let stdout_before = String::from_utf8(doctor_before.get_output().stdout.clone()).unwrap();
    assert!(stdout_before.contains("unversioned_extractor: 1"));

    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "generate",
            "--project",
            "demo",
            "--refresh-stale",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("refreshed 1"));

    let verify_after = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "verify",
            "--all",
        ])
        .assert()
        .success();
    let stdout_after = String::from_utf8(verify_after.get_output().stdout.clone()).unwrap();
    assert!(stdout_after.contains("unversioned_extractor: 0"));
}

#[test]
fn generate_without_refresh_stale_leaves_an_unversioned_snapshot_in_place() {
    let work = tempdir().unwrap();
    let repo_dir = work.path().join("repo");
    init_repo_with_erlang_module_and_tag(&repo_dir, "v1.0.0");
    let snap_dir = work.path().join("snapshots");
    fs::create_dir_all(&snap_dir).unwrap();
    write_snapshot_with_extractor_version(&snap_dir, "demo", "v1.0.0", "");
    let cfg = write_git_project_config(work.path(), &snap_dir, &repo_dir, "demo");

    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("skipped 1"));

    let verify_after = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "snapshots",
            "verify",
            "--all",
        ])
        .assert()
        .code(3);
    let stdout_after = String::from_utf8(verify_after.get_output().stdout.clone()).unwrap();
    assert!(stdout_after.contains("unversioned_extractor: 1"));
}
