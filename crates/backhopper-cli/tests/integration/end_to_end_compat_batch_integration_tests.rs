// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as Std;

use tempfile::{NamedTempFile, TempDir};

use crate::helpers::cli::{run, run_fails, run_succeeds, run_with_stdin, stderr, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

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

fn write_series_config(dir: &Path, repo: &Path, snapshot_dir: &Path) -> PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl", "include/**/*.hrl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "stable"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
]
"#,
        toml_path(snapshot_dir),
        toml_path(repo),
    );
    let p = dir.join("backhopper.toml");
    fs::write(&p, body).unwrap();
    p
}

fn build_two_commit_repo() -> (GitRepoFixture, TempDir, String) {
    let workdir = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
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

fn discover_snapshots(cfg: &Path) {
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
}

fn batch_args<'a>(cfg: &'a Path, repo: &'a Path, commits: &'a Path) -> Vec<&'a str> {
    vec![
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "text",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]
}

#[test]
fn batch_reads_commits_from_file_and_emits_one_row_per_pair() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "# example commits").unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    let out = stdout(&run(batch_args(&cfg, repo.dir.path(), commits.path())));
    assert!(
        out.contains("compatible="),
        "expected key=value rows, got {out}"
    );
    assert!(
        out.contains(&head_sha),
        "expected commit sha in output, got {out}"
    );
    assert!(out.contains("stable"), "expected series name in output");
}

#[test]
fn batch_skips_blank_lines_and_comments() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "# a comment").unwrap();
    writeln!(commits).unwrap();
    writeln!(commits, "   ").unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    writeln!(commits, "# trailing comment").unwrap();
    // one JSONL row per pair, so the line count proves the blanks and
    // comments were skipped
    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "summary",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]));
    let non_empty: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        non_empty.len(),
        1,
        "expected one commit line, got {non_empty:?}"
    );
}

#[test]
fn batch_rejects_missing_commits_file_with_io_error() {
    let (repo, work, _) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let nonexistent = work.path().join("not_there.txt");
    let err = stderr(&run_fails([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        nonexistent.to_str().unwrap(),
    ]));
    assert!(
        err.to_lowercase().contains("no such file") || err.contains("not found"),
        "expected io-flavoured error, got: {err}"
    );
}

#[test]
fn batch_rejects_empty_commits_file_with_clear_error() {
    let (repo, work, _) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let commits = NamedTempFile::new().unwrap();
    let err = stderr(&run_fails([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]));
    assert!(err.contains("no commits"), "stderr was: {err}");
}

#[test]
fn batch_accepts_comma_separated_series_list() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    // Two series, same pin set, to verify --series a,b parses.
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths = ["src/**/*.erl"]
[[project]]
name = "demo"
git_url = "{}"
[[series]]
name = "stable"
pins = [{{ project = "demo", tag = "v1.0.0" }}]
[[series]]
name = "lts"
pins = [{{ project = "demo", tag = "v1.0.0" }}]
"#,
        toml_path(&snap),
        toml_path(repo.dir.path()),
    );
    let cfg = work.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ])
    .success();
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "summary",
        "check",
        "batch",
        "--series",
        "stable,lts",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]));
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        2,
        "expected two rows (one per series), got: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("stable")));
    assert!(lines.iter().any(|l| l.contains("lts")));
}

#[test]
fn batch_accepts_short_sha_prefixes_in_commits_file() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}", &head_sha[..10]).unwrap();
    let out = stdout(&run(batch_args(&cfg, repo.dir.path(), commits.path())));
    assert!(
        out.contains("compatible="),
        "expected batch summary, got: {out}"
    );
}

#[test]
fn batch_accepts_trailing_inline_annotation_after_hash() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}  # [GO] backport candidate", &head_sha[..10]).unwrap();
    let out = stdout(&run(batch_args(&cfg, repo.dir.path(), commits.path())));
    assert!(
        out.contains("compatible="),
        "expected batch summary, got: {out}"
    );
}

#[test]
fn batch_strips_leading_utf8_bom_from_commits_file() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    write!(commits, "\u{FEFF}").unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    let out = stdout(&run(batch_args(&cfg, repo.dir.path(), commits.path())));
    assert!(out.contains("compatible="), "expected summary, got: {out}");
}

#[test]
fn batch_reports_line_number_on_malformed_commit_entry() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    writeln!(commits, "{head_sha}").unwrap();
    writeln!(commits, "bogus").unwrap();
    let err = stderr(&run(batch_args(&cfg, repo.dir.path(), commits.path())).failure());
    assert!(
        err.contains("line 3"),
        "expected line number 3 in error, got: {err}"
    );
}

#[test]
fn batch_reads_commits_from_stdin_when_path_is_dash() {
    let (repo, work, head_sha) = build_two_commit_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_series_config(work.path(), repo.dir.path(), &snap);
    discover_snapshots(&cfg);
    let dash_path = PathBuf::from("-");
    let assert = run_with_stdin(
        batch_args(&cfg, repo.dir.path(), &dash_path),
        &format!("{head_sha}\n"),
    );
    let out = stdout(&assert);
    assert!(out.contains(&head_sha[..7]));
    assert!(out.contains("compatible="));
}
