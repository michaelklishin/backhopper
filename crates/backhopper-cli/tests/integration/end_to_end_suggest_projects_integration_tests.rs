// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `check --suggest-projects` and
//! `--write-suggestions`.

use std::io::Write;

use assert_cmd::Command;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_TRACKED: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

fn fresh_workspace() -> (FixtureRepo, TempDir, std::path::PathBuf) {
    let work = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_TRACKED);
    repo.commit("init");
    repo.tag("v1.0.0");
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
    (repo, work, cfg)
}

const UNTRACKED_PATCH: &str = "\
diff --git a/deps/other/src/other.erl b/deps/other/src/other.erl
--- a/deps/other/src/other.erl
+++ b/deps/other/src/other.erl
@@ -1,1 +1,3 @@
 -module(other).
+a() -> rabbit_misc:format(\"hi\", []).
+b(M) -> rabbit_amqp_util:to_binary(M).
";

#[test]
fn suggest_projects_emits_a_toml_stub_for_inferred_project() {
    let (_repo, _work, cfg) = fresh_workspace();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(UNTRACKED_PATCH.as_bytes()).unwrap();
    let out = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--suggest-projects",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("Suggested [[project]] stubs"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("name       = \"rabbit\""),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("rabbit_misc"), "stdout: {stdout}");
}

#[test]
fn suggest_projects_in_json_envelope() {
    let (_repo, _work, cfg) = fresh_workspace();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(UNTRACKED_PATCH.as_bytes()).unwrap();
    let out = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "json",
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--suggest-projects",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let body: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let suggestions = &body["data"]["project_suggestions"];
    assert!(
        suggestions.is_array(),
        "expected project_suggestions array, got {body}"
    );
    let arr = suggestions.as_array().unwrap();
    assert!(!arr.is_empty(), "should have at least one suggestion");
    assert_eq!(arr[0]["name"], "rabbit");
}

#[test]
fn summary_only_overrides_suggest_projects() {
    // `--summary-only` promises a single-line output. `--suggest-projects`
    // must not violate that even when both are set.
    let (_repo, _work, cfg) = fresh_workspace();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(UNTRACKED_PATCH.as_bytes()).unwrap();
    let out = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--summary-only",
            "--suggest-projects",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let non_empty: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        non_empty.len(),
        1,
        "summary-only must remain one line even with suggestions on: {non_empty:?}"
    );
}

#[test]
fn write_suggestions_appends_project_stubs_to_config() {
    let (_repo, _work, cfg) = fresh_workspace();
    let before = std::fs::read_to_string(&cfg).unwrap();
    assert!(!before.contains("rabbit"));

    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(UNTRACKED_PATCH.as_bytes()).unwrap();
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "patch",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--suggest-projects",
            "--write-suggestions",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let after = std::fs::read_to_string(&cfg).unwrap();
    assert!(after.contains("rabbit"), "config now: {after}");
    assert!(
        after.contains("TODO"),
        "git_url should be a TODO placeholder"
    );
}
