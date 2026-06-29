// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::io::Write;

use assert_cmd::Command;
use tempfile::{NamedTempFile, TempDir};

use backhopper_test_support::GitRepoFixture;

const HOST_MOD_V1: &str = "-module(host_mod).\n-export([util/1]).\nutil(X) -> X.\n";

const FEATURE_V1: &str = "-module(feature).\n-export([go/0]).\ngo() -> ok.\n";

const PATCH_REFERENCING_HOST_FN: &str = "\
diff --git a/src/feature.erl b/src/feature.erl
--- a/src/feature.erl
+++ b/src/feature.erl
@@ -1,3 +1,3 @@
 -module(feature).
 -export([go/0]).
-go() -> ok.
+go() -> host_mod:util(1).
";

const PATCH_REFERENCING_MISSING_HOST_FN: &str = "\
diff --git a/src/feature.erl b/src/feature.erl
--- a/src/feature.erl
+++ b/src/feature.erl
@@ -1,3 +1,3 @@
 -module(feature).
 -export([go/0]).
-go() -> ok.
+go() -> host_mod:not_yet_introduced(1).
";

fn write_self_config(work: &std::path::Path) -> std::path::PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name = "host"
kind = "self"

[[series]]
name = "host-main"
pins = [{{ project = "host", branch = "main" }}]
"#,
        work.join("snapshots").display(),
    );
    let cfg = work.join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn setup_self_repo() -> (GitRepoFixture, TempDir, std::path::PathBuf) {
    let workdir = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
    repo.write_file("src/host_mod.erl", HOST_MOD_V1);
    repo.write_file("src/feature.erl", FEATURE_V1);
    repo.commit("seed");
    fs::create_dir_all(workdir.path().join("snapshots")).unwrap();
    let cfg = write_self_config(workdir.path());
    (repo, workdir, cfg)
}

#[test]
fn config_with_kind_self_loads_and_doctor_succeeds() {
    let (repo, _work, cfg) = setup_self_repo();
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "projects",
            "list",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("host"));
    let _ = repo;
}

#[test]
fn check_patch_against_self_branch_reports_compatible_when_function_exists() {
    let (repo, _work, cfg) = setup_self_repo();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(PATCH_REFERENCING_HOST_FN.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--series",
            "host-main",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("compatible: 1"),
        "expected compatible: 1, got: {stdout}"
    );
}

#[test]
fn check_patch_against_self_branch_emits_missing_prereq_when_function_absent() {
    let (repo, _work, cfg) = setup_self_repo();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(PATCH_REFERENCING_MISSING_HOST_FN.as_bytes())
        .unwrap();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--series",
            "host-main",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            pf.path().to_str().unwrap(),
        ])
        .assert()
        // Exit 2: Incompatible (MissingPrereq is blocking).
        .code(3);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("MissingPrereq"),
        "expected MissingPrereq reason, got: {stdout}"
    );
    assert!(
        stdout.contains("host_mod:not_yet_introduced"),
        "expected the missing MFA in the row, got: {stdout}"
    );
}

#[test]
fn check_patch_fails_clearly_when_self_pin_cwd_is_not_a_git_repo() {
    // pointing `--repo-dir-path` at a non-repo must surface a clear `git error: repository open failed`
    let (_repo, _work, cfg) = setup_self_repo();
    let non_repo = TempDir::new().unwrap();
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(PATCH_REFERENCING_HOST_FN.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--series",
            "host-main",
            "--repo-dir-path",
            non_repo.path().to_str().unwrap(),
            pf.path().to_str().unwrap(),
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        stderr.contains("repository open failed") || stderr.contains("not a git repository"),
        "expected git error about repo open, got: {stderr}"
    );
}

#[test]
fn self_pin_docs_only_patch_stays_inapplicable_not_downgraded_to_compatible() {
    // regression: the prereq promoter once rewrote Inapplicable to Compatible when there were no `MissingSymbol` reasons
    let (repo, _work, cfg) = setup_self_repo();
    // seed README.md so `FileAbsent` does not fire: the docs-only patch is then flipped to `Inapplicable`
    repo.write_file("README.md", "old\n");
    repo.commit("add readme");
    let docs_only = "\
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,1 +1,2 @@
 old
+new
";
    let mut pf = NamedTempFile::new().unwrap();
    pf.write_all(docs_only.as_bytes()).unwrap();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "--formatter",
            "text",
            "check",
            "patch",
            "--series",
            "host-main",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            pf.path().to_str().unwrap(),
        ])
        .assert();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("inapplicable: 1"),
        "expected self-pin to remain inapplicable on a docs-only patch, got: {stdout}"
    );
}
