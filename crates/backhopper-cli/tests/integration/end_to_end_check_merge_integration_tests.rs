// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::process::Command as Std;

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_BASE: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

const ERL_BRANCH: &str = r#"
-module(demo_mod).
-export([greet/1, greet/2]).
greet(Name) -> Name.
greet(_, _) -> demo_mod:greet(<<"x">>).
"#;

fn run_git(repo: &std::path::Path, args: &[&str]) -> String {
    let out = Std::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn build_repo_with_merge() -> (FixtureRepo, TempDir, String) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_BASE);
    repo.commit("base");
    repo.tag("v1.0.0");
    let main_sha = run_git(repo.dir.path(), &["rev-parse", "HEAD"]);

    run_git(repo.dir.path(), &["checkout", "-q", "-b", "feature"]);
    repo.write_file("src/demo_mod.erl", ERL_BRANCH);
    repo.commit("feature work");

    run_git(repo.dir.path(), &["checkout", "-q", "main"]);
    run_git(
        repo.dir.path(),
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e",
            "merge",
            "--no-ff",
            "-m",
            "merge feature",
            "feature",
        ],
    );
    let merge_sha = run_git(repo.dir.path(), &["rev-parse", "HEAD"]);
    assert_ne!(merge_sha, main_sha, "merge should create a new commit");
    (repo, workdir, merge_sha)
}

fn discover(cfg: &std::path::Path) {
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
}

#[test]
fn check_commit_on_merge_sha_errors_with_hint_to_check_merge() {
    let (repo, work, merge_sha) = build_repo_with_merge();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover(&cfg);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "commit",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            &merge_sha,
        ])
        .assert()
        .failure();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("merge commit") && stderr.contains("check merge"),
        "expected hint pointing at 'check merge', got stderr: {stderr}"
    );
}

#[test]
fn check_merge_evaluates_the_merge_diff() {
    let (repo, work, merge_sha) = build_repo_with_merge();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover(&cfg);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "merge",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            &merge_sha,
        ])
        .assert();
    let output = assert.get_output();
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Compatible and Inapplicable both exit 0; only needs-attention exits 3.
    assert_eq!(code, 0, "stdout: {stdout}");
    assert!(
        !stdout.contains("incompatible: 1"),
        "merge diff should not be Incompatible. stdout: {stdout}"
    );
}

#[test]
fn check_merge_accepts_short_sha_prefix() {
    let (repo, work, merge_sha) = build_repo_with_merge();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover(&cfg);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "merge",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            &merge_sha[..10],
        ])
        .assert();
    let code = assert.get_output().status.code().unwrap_or(-1);
    assert_eq!(code, 0, "got unexpected exit {code}");
}

#[test]
fn check_range_merge_commit_accepts_short_sha_prefix() {
    let (repo, work, merge_sha) = build_repo_with_merge();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover(&cfg);
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "check",
            "range",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "--merge-commit",
            &merge_sha[..10],
            "--formatter",
            "text",
        ])
        .assert();
    let code = assert.get_output().status.code().unwrap_or(-1);
    assert_eq!(code, 0, "got unexpected exit {code}");
}
