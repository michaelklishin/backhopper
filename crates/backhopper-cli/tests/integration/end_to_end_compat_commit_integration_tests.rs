use std::process::Command as Std;

use tempfile::TempDir;

use crate::helpers::cli::{run, run_succeeds, stderr, stdout};
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

fn discover_snapshots(cfg: &std::path::Path) {
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
}

fn commit_args<'a>(
    cfg: &'a std::path::Path,
    repo: &'a std::path::Path,
    sha: &'a str,
    extra: &[&'a str],
) -> Vec<&'a str> {
    let mut args = vec![
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--project",
        "demo",
        "--tag",
        "v1.0.0",
        "--repo-dir-path",
        repo.to_str().unwrap(),
        "--formatter",
        "text",
    ];
    args.extend_from_slice(extra);
    args.push(sha);
    args
}

#[test]
fn compat_commit_accepts_short_sha() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let out = stdout(&run(commit_args(
        &cfg,
        repo.dir.path(),
        &head_sha[..10],
        &[],
    )));
    assert!(
        out.contains("compatible") || out.contains("incompatible"),
        "expected verdict in output, got {}",
        out
    );
}

#[test]
fn compat_commit_rejects_unknown_sha_with_clear_error() {
    let (repo, work, _) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let err = stderr(&run(commit_args(&cfg, repo.dir.path(), "deadbeef", &[])).failure());
    assert!(err.contains("deadbeef"), "stderr was: {}", err);
    assert!(
        err.contains("not found in repository"),
        "expected workflow hint in error, got: {}",
        err
    );
    assert!(
        err.contains("git fetch"),
        "expected `git fetch` hint in error, got: {}",
        err
    );
    assert!(
        err.contains(repo.dir.path().to_str().unwrap()),
        "expected repo path in error, got: {}",
        err
    );
}

#[test]
fn compat_commit_summary_only_prints_single_kv_line() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let out = stdout(&run(commit_args(
        &cfg,
        repo.dir.path(),
        &head_sha,
        &["--summary-only"],
    )));
    assert!(
        out.contains("compatible=") && out.contains("incompatible="),
        "expected key=value summary, got {}",
        out
    );
    let non_empty: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        non_empty.len(),
        1,
        "summary-only must collapse to a single line, got: {:?}",
        non_empty
    );
}

#[test]
fn compat_commit_runs_against_local_repo() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let out = stdout(&run(commit_args(&cfg, repo.dir.path(), &head_sha, &[])));
    assert!(
        out.contains("compatible") || out.contains("incompatible"),
        "expected verdict in output, got {}",
        out
    );
}
