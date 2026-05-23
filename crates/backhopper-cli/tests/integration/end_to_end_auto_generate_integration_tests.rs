// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `--auto-generate` on `check commit`. We build a
//! tiny FixtureRepo, do not generate snapshots ahead of time, and run
//! `check commit` once without the flag (expects MissingSnapshots) and
//! once with it (expects the snapshot to be written and the verdict to
//! return).

use std::process::Command as Std;

use tempfile::TempDir;

use crate::helpers::cli::{run, stderr, stdout};
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
greet(_, _) -> demo_mod:greet(<<"x">>).
"#;

fn build_repo() -> (FixtureRepo, TempDir, String) {
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
fn check_commit_without_auto_generate_returns_missing_snapshots_error() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    std::fs::create_dir_all(&snap).unwrap();
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    let assert = run([
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
        &head_sha,
    ])
    .failure();
    let err = stderr(&assert);
    assert!(
        err.contains("snapshots missing"),
        "stderr should advertise MissingSnapshots, got: {err}"
    );
    assert!(
        err.contains("demo @ v1.0.0"),
        "stderr should list the missing pin, got: {err}"
    );
    assert!(
        err.contains("hint:"),
        "stderr should carry a hint, got: {err}"
    );
    assert!(
        err.contains("backhopper snapshots generate"),
        "hint should point at `snapshots generate`, got: {err}"
    );
}

#[test]
fn check_commit_with_auto_generate_writes_the_snapshot_and_evaluates() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    std::fs::create_dir_all(&snap).unwrap();
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    let assert = run([
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
        "--auto-generate",
        &head_sha,
    ]);
    let text = stdout(&assert);
    assert!(
        text.contains("compatible") || text.contains("incompatible"),
        "auto-generate should let the check complete, got: {text}"
    );
    let snapshot_file = snap.join("demo").join("v1.0.0.api.txt");
    assert!(
        snapshot_file.exists(),
        "snapshot must be written on disk by --auto-generate"
    );
}
