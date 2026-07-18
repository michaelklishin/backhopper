// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The recall-superset property, grounded against real git: whenever an
//! actual cherry-pick conflicts, the preimage predictor must flag it.
//! Each scenario builds one repo, takes git's conflict outcome as the
//! oracle, then asserts the CLI verdict carries a conflict reason.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

fn write_config(
    workdir: &TempDir,
    repo: &GitRepoFixture,
    snapshot_dir: &Path,
) -> std::path::PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["deps/**/src/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "demo-series"
pins = [
    {{ project = "demo", tag = "source-v1" }},
]
"#,
        toml_path(snapshot_dir),
        toml_path(repo.dir.path()),
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    cfg
}

fn generate_snapshot(cfg_path: &Path) {
    run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "demo-series",
    ])
    .success();
}

/// Build a repo whose candidate commit takes `base` to `candidate`, with
/// a `target` branch that diverged from `base`. Returns whether the real
/// cherry-pick conflicts and whether the CLI verdict flags a conflict.
fn outcome(base: &str, candidate: &str, target: &str) -> (bool, bool) {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let repo = GitRepoFixture::new();
    repo.write_file("deps/demo/src/x.erl", base);
    repo.commit("base");

    repo.checkout_new_branch("target");
    repo.write_file("deps/demo/src/x.erl", target);
    repo.commit("target diverges");

    repo.checkout("main");
    repo.write_file("deps/demo/src/x.erl", candidate);
    repo.commit("candidate");
    let cand = repo.head_sha();
    repo.tag("source-v1");

    repo.checkout("target");
    let conflicted = repo.cherry_pick_conflicts(&cand);
    repo.checkout("main");

    let cfg_path = write_config(&workdir, &repo, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--target-ref",
        "target",
        &cand,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let flagged = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .is_some_and(|reasons| {
            reasons.iter().any(|r| {
                matches!(
                    r["kind"].as_str(),
                    Some("preimage_missing" | "postimage_collision")
                )
            })
        });
    (conflicted, flagged)
}

fn assert_superset(base: &str, candidate: &str, target: &str) {
    let (conflicted, flagged) = outcome(base, candidate, target);
    assert!(
        !conflicted || flagged,
        "git conflicted but the predictor did not flag it"
    );
}

#[test]
fn modify_modify_conflict_is_flagged() {
    let (conflicted, flagged) = outcome(
        "-module(x).\nv = old.\ntail.\n",
        "-module(x).\nv = candidate.\ntail.\n",
        "-module(x).\nv = target.\ntail.\n",
    );
    assert!(conflicted, "this scenario must conflict in git");
    assert!(flagged, "the predictor must flag the conflict");
}

#[test]
fn trailing_add_add_conflict_is_flagged() {
    let (conflicted, flagged) = outcome(
        "-module(x).\n",
        "-module(x).\ncandidate_tail.\n",
        "-module(x).\ntarget_tail.\n",
    );
    assert!(conflicted, "this scenario must conflict in git");
    assert!(flagged, "the predictor must flag the conflict");
}

// a change the target shifted but did not touch applies cleanly: only conflicts must be flagged
#[test]
fn a_clean_shifted_pick_respects_the_superset_property() {
    assert_superset(
        "-module(x).\nv = old.\ntail.\n",
        "-module(x).\nv = candidate.\ntail.\n",
        "%% prepended on target.\n-module(x).\nv = old.\ntail.\n",
    );
}

#[test]
fn a_disjoint_change_respects_the_superset_property() {
    assert_superset(
        "-module(x).\na = 1.\nb = 1.\n",
        "-module(x).\na = 2.\nb = 1.\n",
        "-module(x).\na = 1.\nb = 9.\n",
    );
}
