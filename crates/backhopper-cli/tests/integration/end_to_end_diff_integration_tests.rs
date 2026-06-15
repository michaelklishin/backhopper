// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::{FixtureRepo, write_config};

const V1: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

const V2: &str = r#"
-module(demo_mod).
-export([greet/1, greet/2]).
greet(Name) -> Name.
greet(_, _) -> ok.
"#;

fn build_two_tag_repo() -> (FixtureRepo, TempDir) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", V1);
    repo.commit("v1");
    repo.tag("v1.0.0");
    repo.write_file("src/demo_mod.erl", V2);
    repo.commit("v2");
    repo.tag("v1.1.0");
    (repo, workdir)
}

#[test]
fn api_diff_reports_added_export() {
    let (repo, work) = build_two_tag_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "project_diff",
            "--project",
            "demo",
            "--from",
            "v1.0.0",
            "--to",
            "v1.1.0",
            "--formatter",
            "text",
        ])
        .assert();
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("greet/2"), "got {stdout}");
}

#[test]
fn snapshots_list_shows_both_tags() {
    let (repo, work) = build_two_tag_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "list",
            "--project",
            "demo",
            "--formatter",
            "text",
        ])
        .assert();
    let stdout = String::from_utf8(assert.success().get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("v1.0.0"));
    assert!(stdout.contains("v1.1.0"));
}
