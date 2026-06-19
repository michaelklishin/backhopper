// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `check batch --formatter json` emits `self_projects`, so a consumer
//! can rebuild the clearance from the envelope.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as Std;

use backhopper_core::model::batch::BatchPayload;
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::cli::{run, run_succeeds, stdout};
use crate::helpers::fixture::FixtureRepo;

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

fn write_config(dir: &Path, repo: &Path, snapshot_dir: &Path) -> PathBuf {
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
        snapshot_dir.display(),
        repo.display(),
    );
    let p = dir.join("backhopper.toml");
    fs::write(&p, body).unwrap();
    p
}

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
fn batch_json_envelope_carries_self_projects_and_reconstructs_a_clearance() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{head_sha}").unwrap();

    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]));

    let envelope: Value = serde_json::from_str(&out).unwrap();
    // No self project is configured, so the field is present and empty,
    // not absent: that is the "always emit" guarantee a consumer relies
    // on to tell an empty set from an old binary.
    let self_projects = envelope["data"]["self_projects"]
        .as_array()
        .unwrap_or_else(|| panic!("self_projects must be present in the batch envelope: {out}"));
    assert!(self_projects.is_empty());

    let payload: BatchPayload = serde_json::from_value(envelope["data"].clone()).unwrap();
    assert!(payload.clearance_self_inferred().is_some());
}

#[test]
fn single_check_json_envelope_carries_self_projects() {
    let (repo, work, head_sha) = build_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);

    let out = stdout(&run([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "commit",
        "--project",
        "demo",
        "--tag",
        "v1.0.0",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        &head_sha,
    ]));

    let envelope: Value = serde_json::from_str(&out).unwrap();
    assert!(
        envelope["data"]["self_projects"]
            .as_array()
            .is_some_and(|projects| projects.is_empty()),
        "single-check envelope must carry an empty self_projects: {out}"
    );
}
