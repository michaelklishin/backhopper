// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_SRC: &str = r#"
-module(demo_mod).
-export([greet/1, greet/2]).
-spec greet(unicode:chardata()) -> ok.
greet(Name) -> io:format("hi ~ts~n", [Name]).
greet(_, _) -> ok.
"#;

fn build_demo_repo() -> (FixtureRepo, TempDir) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_SRC);
    repo.commit("first");
    repo.tag("v1.0.0");
    (repo, workdir)
}

#[test]
fn version_command_prints_version() {
    let mut cmd = Command::cargo_bin("backhopper").unwrap();
    let assert = cmd.args(["version", "--formatter", "text"]).assert();
    assert
        .success()
        .stdout(predicates::str::contains("backhopper "));
}

#[test]
fn help_text_contains_command_groups() {
    let mut cmd = Command::cargo_bin("backhopper").unwrap();
    let assert = cmd.args(["--help"]).assert();
    let output = assert.success().get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    for grp in ["projects", "series", "snapshots", "check", "shell", "xref"] {
        assert!(stdout.contains(grp), "help missing {grp}: {stdout}");
    }
}

#[test]
fn config_validate_accepts_well_formed_file() {
    let (repo, work) = build_demo_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    let mut cmd = Command::cargo_bin("backhopper").unwrap();
    cmd.args([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "config",
        "validate",
        "--formatter",
        "text",
    ])
    .assert()
    .success()
    .stdout(predicates::str::contains("ok"));
}

#[test]
fn discover_writes_snapshot_for_each_tag() {
    let (repo, work) = build_demo_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    let mut cmd = Command::cargo_bin("backhopper").unwrap();
    cmd.args([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ])
    .assert()
    .success();
    let snapshot = snap.join("demo").join("v1.0.0.api.txt");
    assert!(
        snapshot.exists(),
        "expected snapshot at {}",
        snapshot.display()
    );
    let body = std::fs::read_to_string(&snapshot).unwrap();
    assert!(body.contains("module demo_mod"));
    assert!(body.contains("export greet/1"));
    assert!(body.contains("export greet/2"));
}

#[test]
fn api_lookup_reports_found_and_missing() {
    let (repo, work) = build_demo_repo();
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
            "lookup",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--mfa",
            "demo_mod:greet/1",
            "--mfa",
            "demo_mod:does_not_exist/0",
            "--formatter",
            "text",
        ])
        .assert();
    let out = assert.code(3).get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("demo_mod:greet/1\tfound"));
    assert!(stdout.contains("demo_mod:does_not_exist/0\tmissing"));
}

#[test]
fn snapshots_show_pretty_prints_canonical_text() {
    let (repo, work) = build_demo_repo();
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
            "show",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--formatter",
            "text",
        ])
        .assert();
    let out = assert.success().get_output().clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("# backhopper snapshot"));
    assert!(stdout.contains("module demo_mod"));
}

#[test]
fn snapshots_verify_passes_on_unchanged_repo() {
    let (repo, work) = build_demo_repo();
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
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "verify",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--formatter",
            "text",
        ])
        .assert()
        .success();
}
