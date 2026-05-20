// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as Std;

use tempfile::{NamedTempFile, TempDir};

use crate::helpers::cli::{run, stderr};
use crate::helpers::fixture::FixtureRepo;

const ERL: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

fn write_partial_config(dir: &Path, repo: &Path, snapshot_dir: &Path) -> PathBuf {
    let body = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir    = "{}"
fallback_branch = "main"
scan_paths      = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[project]]
name    = "missing_one"
git_url = "{}"

[[series]]
name = "stable"
pins = [
    {{ project = "demo", tag = "v1.0.0" }},
]
"#,
        snapshot_dir.display(),
        repo.display(),
        repo.display(),
    );
    let p = dir.join("backhopper.toml");
    fs::write(&p, body).unwrap();
    p
}

fn build_repo() -> (FixtureRepo, TempDir, String) {
    let workdir = TempDir::new().unwrap();
    let repo = FixtureRepo::new();
    repo.write_file("src/demo_mod.erl", ERL);
    repo.commit("v1");
    repo.tag("v1.0.0");
    repo.write_file("src/demo_mod.erl", &format!("{}\n", ERL));
    repo.commit("v2");
    let head = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    (repo, workdir, head_sha)
}

#[test]
fn series_with_uncovered_project_emits_warning() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_partial_config(work.path(), repo.dir.path(), &snap);
    run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ])
    .success();
    let err = stderr(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        &head_sha,
    ]));
    assert!(
        err.contains("series stable") && err.contains("missing_one"),
        "expected coverage warning on stderr, got: {}",
        err
    );
}

#[test]
fn series_with_full_coverage_emits_no_warning() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
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
"#,
        snap.display(),
        repo.dir.path().display(),
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
    let err = stderr(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        &head_sha,
    ]));
    assert!(
        !err.contains("no pin"),
        "expected no coverage warning, got: {}",
        err
    );
}

#[test]
fn series_coverage_warning_fires_for_batch_too() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_partial_config(work.path(), repo.dir.path(), &snap);
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
    writeln!(commits, "{}", head_sha).unwrap();
    let err = stderr(&run([
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
    assert!(
        err.contains("missing_one"),
        "expected coverage warning during batch, got: {}",
        err
    );
}