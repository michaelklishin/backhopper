// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The doctor's series-pin coverage warning: a tracked project a
//! series carries no pin for silently defeats dep-snapshot lookup,
//! so the doctor names the pair; `untracked_projects` records the
//! deliberate cases.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::snapshot::format;

use crate::helpers::cli::{run, stdout};

fn write_config(dir: &Path, body: &str) -> PathBuf {
    let cfg = dir.join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn write_snapshot_with_extractor_version(
    snapshot_dir: &Path,
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
    let dir = snapshot_dir.join(project);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{tag}.api.txt")),
        format::to_string(&snap).unwrap(),
    )
    .unwrap();
}

const GAP_CONFIG: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name    = "cowboy"
git_url = "/nonexistent/cowboy.git"

[[project]]
name    = "cowlib"
git_url = "/nonexistent/cowlib.git"

[[series]]
name = "rabbitmq-3.13"
pins = [
    { project = "cowboy", tag = "2.12.0" },
]
"#;

const OPTED_OUT_CONFIG: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name    = "cowboy"
git_url = "/nonexistent/cowboy.git"

[[project]]
name    = "cowlib"
git_url = "/nonexistent/cowlib.git"

[[series]]
name = "rabbitmq-3.13"
pins = [
    { project = "cowboy", tag = "2.12.0" },
]
untracked_projects = ["cowlib"]
"#;

fn doctor_json(cfg: &Path) -> Value {
    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "doctor",
    ]));
    serde_json::from_str(&out).unwrap()
}

#[test]
fn unpinned_tracked_project_surfaces_as_a_series_gap() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), GAP_CONFIG);
    fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let body = doctor_json(&cfg);
    let gaps = body["data"]["series_pin_gaps"].as_array().unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0]["series"], "rabbitmq-3.13");
    assert_eq!(gaps[0]["project"], "cowlib");
}

#[test]
fn gap_warning_renders_with_a_remediation() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), GAP_CONFIG);
    fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let text = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "doctor",
    ]));
    assert!(
        text.contains("series rabbitmq-3.13: no pin for tracked project cowlib"),
        "missing gap line: {text}"
    );
    assert!(
        text.contains("series sync merge --series-name rabbitmq-3.13"),
        "{text}"
    );
}

#[test]
fn untracked_projects_opt_out_silences_exactly_its_pair() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), OPTED_OUT_CONFIG);
    fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    let body = doctor_json(&cfg);
    let gaps = body["data"]["series_pin_gaps"].as_array().unwrap();
    assert!(gaps.is_empty(), "opt-out did not silence the gap: {gaps:?}");
}

#[test]
fn unknown_project_in_opt_out_is_a_config_error() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(
        tmp.path(),
        &OPTED_OUT_CONFIG.replace(
            "untracked_projects = [\"cowlib\"]",
            "untracked_projects = [\"nonesuch\"]",
        ),
    );
    fs::create_dir_all(tmp.path().join("snapshots")).unwrap();
    run(["--config-file-path", cfg.to_str().unwrap(), "doctor"]).failure();
}

const STALE_CONFIG: &str = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name    = "gen_batch_server"
git_url = "/nonexistent/gen_batch_server.git"

[[series]]
name = "rabbitmq-4.1"
pins = [
    { project = "gen_batch_server", tag = "v0.8.8" },
]
"#;

#[test]
fn doctor_flags_a_stale_extractor_pin_with_partial_success_exit() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), STALE_CONFIG);
    let snapshots = tmp.path().join("snapshots");
    // Generated by extractor version 1, older than the running binary.
    write_snapshot_with_extractor_version(&snapshots, "gen_batch_server", "v0.8.8", "1");
    let a = run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "doctor",
    ]);
    let text = stdout(&a);
    a.code(3);
    assert!(text.contains("STALE"), "stdout: {text}");
    assert!(text.contains("stale-extractor"), "stdout: {text}");
    assert!(text.contains("snapshots generate"), "stdout: {text}");
}

#[test]
fn doctor_json_carries_the_stale_extractor_status() {
    let tmp = TempDir::new().unwrap();
    let cfg = write_config(tmp.path(), STALE_CONFIG);
    let snapshots = tmp.path().join("snapshots");
    write_snapshot_with_extractor_version(&snapshots, "gen_batch_server", "v0.8.8", "1");
    let body = doctor_json(&cfg);
    assert_eq!(body["data"]["totals"]["stale_extractor"], 1);
    assert_eq!(body["data"]["totals"]["present"], 1);
    assert_eq!(body["data"]["totals"]["missing"], 0);
    let snapshot = &body["data"]["series"][0]["pins"][0]["snapshot"];
    assert_eq!(snapshot["status"], "stale");
    assert_eq!(snapshot["stored"], "1");
}
