// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;
use std::process::Command as Std;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run_succeeds, stdout};
use crate::helpers::fixture::{FixtureRepo, write_config};

const ERL_V1: &str = "-module(demo_mod).\n-export([greet/1]).\ngreet(N) -> N.\n";
const ERL_V2: &str =
    "-module(demo_mod).\n-export([greet/1, farewell/1]).\ngreet(N) -> N.\nfarewell(N) -> N.\n";

fn build_two_tag_repo() -> (FixtureRepo, TempDir, String) {
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

fn generate_snapshots(cfg: &Path) {
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
}

#[test]
fn bisect_commit_accepts_short_sha_and_emits_resolved_full_sha_in_envelope() {
    let (repo, work, head_sha) = build_two_tag_repo();
    let snap = work.path().join("snapshots");
    let cfg = write_config(work.path(), repo.dir.path(), &snap);
    generate_snapshots(&cfg);
    let out = stdout(&run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "bisect",
        "commit",
        "--project",
        "demo",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        &head_sha[..10],
    ]));
    let v: Value = serde_json::from_str(&out).expect("json envelope");
    assert_eq!(v["command"], "bisect commit");
    assert_eq!(v["data"]["commit"], head_sha);
}
