// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::FixtureRepo;

const COMPONENTS_MK: &str = r#"
PROJECT = rabbit
dep_cowboy = hex 2.13.0
dep_khepri = hex 0.17.5
dep_osiris = git https://github.com/rabbitmq/osiris v1.10.3
dep_ra = hex 2.16.7
dep_rabbitmq_lvc_exchange = $(call community_dep,rabbitmq-lvc-exchange)
"#;

fn build_components_repo() -> (FixtureRepo, TempDir) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("rabbitmq-components.mk", COMPONENTS_MK);
    repo.commit("v1");
    (repo, workdir)
}

fn write_full_config(workdir: &Path, snapshot_dir: &Path) -> PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name    = "ra"
git_url = "/tmp/ra.git"

[[project]]
name    = "khepri"
git_url = "/tmp/khepri.git"

[[project]]
name    = "osiris"
git_url = "/tmp/osiris.git"

[[project]]
name       = "cowboy"
git_url    = "/tmp/cowboy.git"
tag_prefix = ""
"#,
        snapshot_dir.display()
    );
    let cfg = workdir.join("backhopper.toml");
    std::fs::write(&cfg, body).unwrap();
    cfg
}

#[test]
fn infer_from_rabbitmq_against_synthetic_components_mk() {
    let (repo, work) = build_components_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_full_config(work.path(), &snap);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "rabbitmq",
            "infer_series",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--branch",
            "main",
            "--formatter",
            "text",
        ])
        .assert();
    let output = assert.success().get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rabbitmq-main"), "got {}", stdout);
    assert!(stdout.contains(r#"project = "ra", tag = "v2.16.7""#));
    assert!(stdout.contains(r#"project = "khepri", tag = "v0.17.5""#));
    assert!(stdout.contains(r#"project = "osiris", tag = "v1.10.3""#));
    assert!(stdout.contains(r#"project = "cowboy", tag = "2.13.0""#));
    assert!(!stdout.contains("rabbitmq_lvc_exchange"));
}

#[test]
fn infer_from_rabbitmq_uses_explicit_name() {
    let (repo, work) = build_components_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_full_config(work.path(), &snap);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "rabbitmq",
            "infer_series",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--branch",
            "main",
            "--name",
            "custom-name",
            "--formatter",
            "text",
        ])
        .assert();
    let output = assert.success().get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("custom-name"));
}
