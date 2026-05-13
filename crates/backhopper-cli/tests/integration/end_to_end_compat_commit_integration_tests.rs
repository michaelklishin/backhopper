use std::process::Command as Std;

use assert_cmd::Command;
use tempfile::TempDir;

use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_V1: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

const ERL_V2: &str = r#"
-module(demo_mod).
-export([greet/1, greet/2]).
greet(Name) -> Name.
greet(_, _) -> demo_mod:greet(<<\"x\">>).
"#;

fn build_two_commit_repo() -> (FixtureRepo, TempDir, String) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL_V1);
    repo.commit("v1");
    repo.tag("v1.0.0");
    repo.write_file("src/demo_mod.erl", ERL_V2);
    repo.commit("v2");
    repo.tag("v1.1.0");
    let head = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    (repo, workdir, head_sha)
}

#[test]
fn compat_commit_accepts_short_sha() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let short = &head_sha[..10];
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "compatibility",
            "commit",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            short,
        ])
        .assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("compatible") || stdout.contains("incompatible"),
        "expected verdict in output, got {}",
        stdout
    );
}

#[test]
fn compat_commit_rejects_unknown_sha_with_clear_error() {
    let (repo, work, _) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "compatibility",
            "commit",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            "deadbeef",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("deadbeef"), "stderr was: {}", stderr);
}

#[test]
fn compat_commit_runs_against_local_repo() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "snapshots",
            "discover",
            "--project",
            "demo",
        ])
        .assert()
        .success();
    let assert = Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "compatibility",
            "commit",
            "--project",
            "demo",
            "--tag",
            "v1.0.0",
            "--repo",
            repo.dir.path().to_str().unwrap(),
            "--formatter",
            "text",
            head_sha.as_str(),
        ])
        .assert();
    let output = assert.get_output().clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("compatible") || stdout.contains("incompatible"),
        "expected verdict in output, got {}",
        stdout
    );
}
