// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::io::Write;

use assert_cmd::Command;
use tempfile::NamedTempFile;

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_V1: &str = "-module(demo_mod).\n-export([greet/1]).\ngreet(Name) -> Name.\n";

const PATCH_COMPATIBLE: &str = "\
diff --git a/src/demo_mod.erl b/src/demo_mod.erl
--- a/src/demo_mod.erl
+++ b/src/demo_mod.erl
@@ -1,3 +1,4 @@
 -module(demo_mod).
 -export([greet/1]).
+example() -> demo_mod:greet(<<\"x\">>).
 greet(Name) -> Name.
";

fn fresh_repo_with_snapshot() -> (FixtureRepo, tempfile::TempDir, std::path::PathBuf) {
    let work = tempfile::TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_V1);
    repo.commit("v1");
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

#[test]
fn terse_writes_one_json_line_with_expected_keys() {
    let (_repo, _work, cfg) = fresh_repo_with_snapshot();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(PATCH_COMPATIBLE.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
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
            "--terse",
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected one line, got: {stdout}");
    let parsed: serde_json::Value =
        serde_json::from_str(lines[0]).expect("terse line should parse as JSON");
    assert!(
        parsed.get("summary").is_some(),
        "missing 'summary': {parsed}"
    );
    assert!(parsed.get("pins").is_some(), "missing 'pins': {parsed}");
    assert!(parsed.get("scope").is_some(), "missing 'scope': {parsed}");
    assert!(parsed.get("exit").is_some(), "missing 'exit': {parsed}");
    assert_eq!(parsed["summary"], "compatible");
    assert_eq!(parsed["pins"], 1);
    assert_eq!(parsed["scope"], "source");
    assert_eq!(parsed["exit"], 0);
}

#[test]
fn terse_reports_docs_only_scope_for_markdown_patch() {
    let (_repo, _work, cfg) = fresh_repo_with_snapshot();
    let body = "\
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,1 +1,2 @@
 old
+new
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(body.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
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
            "--terse",
            pf.path().to_str().unwrap(),
        ])
        .assert();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.lines().next().unwrap()).expect("one JSON line");
    // Single .md file: scope is "docs_only" regardless of per-pin reason.
    assert_eq!(parsed["scope"], "docs_only");
    assert_eq!(parsed["pins"], 1);
}

#[test]
fn terse_reports_schema_only_scope_for_schema_patch() {
    let (_repo, _work, cfg) = fresh_repo_with_snapshot();
    let body = "\
diff --git a/priv/schema/demo.schema b/priv/schema/demo.schema
--- a/priv/schema/demo.schema
+++ b/priv/schema/demo.schema
@@ -1,1 +1,2 @@
 old
+new
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(body.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
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
            "--terse",
            pf.path().to_str().unwrap(),
        ])
        .assert();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.lines().next().unwrap()).expect("one JSON line");
    assert_eq!(parsed["scope"], "schema_only");
    assert_eq!(parsed["pins"], 1);
}
