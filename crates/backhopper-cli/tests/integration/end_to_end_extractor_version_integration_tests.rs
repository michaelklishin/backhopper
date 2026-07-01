// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;
use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
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
fn verify_all_passes_when_extractor_version_is_empty() {
    // Pre-versioning snapshots have an empty `extractor_version` field and
    // must not produce stale warnings; the field is missing-tolerant.
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
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("stale_extractor: 0"),
        "expected stale_extractor: 0, got: {stdout}"
    );
}
