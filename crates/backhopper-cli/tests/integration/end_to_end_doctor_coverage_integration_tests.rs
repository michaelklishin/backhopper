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

use crate::helpers::cli::{run, stdout};

fn write_config(dir: &Path, body: &str) -> PathBuf {
    let cfg = dir.join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
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
