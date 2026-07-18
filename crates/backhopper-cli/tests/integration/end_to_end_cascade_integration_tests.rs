// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `check cascade`: one commit set,
//! one series per leg, each leg's target from its series stanza.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_with_env, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

struct CascadeFixture {
    workdir: TempDir,
    cfg_path: PathBuf,
    source: GitRepoFixture,
    sha: String,
}

fn build_fixture() -> CascadeFixture {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    fs::create_dir_all(&snapshot_dir).unwrap();
    // the pinned dep is a separate repo with the standalone layout the pin scope rewrites monorepo paths to
    let dep = GitRepoFixture::new();
    dep.write_file(
        "src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    dep.commit("seed caller");
    dep.tag("source-v1");
    // The source monorepo vendors the dep; the pick lands here.
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    source.commit("seed vendored caller");
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\nf() -> helper:ping().\n",
    );
    source.commit("call helper:ping/0");
    let sha = source.head_sha();

    // Leg A's target has the export; leg B's does not.
    let target_a = GitRepoFixture::new();
    target_a.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    target_a.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([ping/0]).\nping() -> pong.\n",
    );
    target_a.commit("seed with ping/0");
    let target_b = GitRepoFixture::new();
    target_b.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    target_b.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([present/0]).\npresent() -> ok.\n",
    );
    target_b.commit("seed without ping/0");

    let body = format!(
        r#"
config_version = 1
[defaults]
snapshot_dir = "{}"
fallback_branch = "main"
scan_paths   = ["src/**/*.erl"]

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "leg-a"
pins = [ {{ project = "demo", tag = "source-v1" }} ]
target_repo_dir_path = "{}"

[[series]]
name = "leg-b"
pins = [ {{ project = "demo", tag = "source-v1" }} ]
target_repo_dir_path = "{}"

[[series]]
name = "no-target"
pins = [ {{ project = "demo", tag = "source-v1" }} ]
"#,
        toml_path(&snapshot_dir),
        toml_path(dep.dir.path()),
        toml_path(target_a.dir.path()),
        toml_path(target_b.dir.path()),
    );
    let cfg_path = workdir.path().join("backhopper.toml");
    fs::write(&cfg_path, body).unwrap();
    run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "snapshots",
        "generate",
        "--series",
        "leg-a",
    ])
    .success();
    // The fixtures outlive the struct through the config's paths.
    std::mem::forget(dep);
    std::mem::forget(target_a);
    std::mem::forget(target_b);
    CascadeFixture {
        workdir,
        cfg_path,
        source,
        sha,
    }
}

fn write_commits_file(f: &CascadeFixture) -> PathBuf {
    let p = f.workdir.path().join("commits.txt");
    fs::write(&p, format!("{}\n", f.sha)).unwrap();
    p
}

fn cascade_args(f: &CascadeFixture, commits: &Path) -> Vec<String> {
    [
        "--config-file-path",
        f.cfg_path.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "leg-a,leg-b",
        "--repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

#[test]
fn a_two_leg_cascade_produces_a_matrix_and_per_leg_blocks() {
    let f = build_fixture();
    let commits = write_commits_file(&f);
    let mut args = cascade_args(&f, &commits);
    args.insert(0, "--formatter".into());
    args.insert(1, "json".into());
    let assert = run(args.iter().map(String::as_str));
    let code = assert.get_output().status.code().unwrap();
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    assert_eq!(env["command"], "check cascade");
    let legs = env["data"]["legs"].as_array().expect("legs array");
    assert_eq!(legs.len(), 2);
    assert_eq!(legs[0]["series"], "leg-a");
    assert_eq!(legs[1]["series"], "leg-b");
    assert_eq!(legs[0]["target_ref"], "HEAD");
    assert_eq!(
        legs[0]["exit_code"], 0,
        "leg-a {:?}",
        legs[0]["batch"]["results"]
    );
    assert_eq!(
        legs[1]["exit_code"], 3,
        "leg-b {:?}",
        legs[1]["batch"]["results"]
    );
    // The per-leg payload is the batch payload verbatim.
    assert_eq!(legs[1]["batch"]["results"][0]["series"], "leg-b");
    let reasons = legs[1]["batch"]["results"][0]["verdict"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("reasons");
    assert!(
        reasons
            .iter()
            .any(|r| r["kind"] == "qualified_call_undefined_on_target"),
        "leg-b must flag the missing export: {reasons:?}"
    );
    // Exit is the worst leg's.
    assert_eq!(code, 3);

    let text_assert = run(cascade_args(&f, &commits).iter().map(String::as_str));
    let text = stdout(&text_assert);
    assert!(text.contains("leg-a"), "matrix header: {text}");
    assert!(text.contains("leg-b"));
    assert!(text.contains("compatible"));
    assert!(text.contains("requires_adaptation"));
    assert!(text.contains("leg leg-a ("), "per-leg block: {text}");
    assert!(text.contains("clearance:"));
}

// run twice with the cache forced on: a leg-2 verdict served from leg 1's entry would flip the second run's cells
#[test]
fn a_second_cached_run_keeps_the_legs_apart() {
    let f = build_fixture();
    let commits = write_commits_file(&f);
    let cache_on = [("BACKHOPPER_FORCE_CACHE", "1")];
    let mut args = cascade_args(&f, &commits);
    args.insert(0, "--formatter".into());
    args.insert(1, "json".into());
    let first = run_with_env(args.iter().map(String::as_str), &cache_on);
    let env1: Value = serde_json::from_str(&stdout(&first)).unwrap();
    let second = run_with_env(args.iter().map(String::as_str), &cache_on);
    let env2: Value = serde_json::from_str(&stdout(&second)).unwrap();
    for env in [&env1, &env2] {
        let legs = env["data"]["legs"].as_array().unwrap();
        assert_eq!(legs[0]["exit_code"], 0, "leg-a stays clean");
        assert_eq!(legs[1]["exit_code"], 3, "leg-b stays flagged");
    }
}

#[test]
fn a_series_without_a_target_errors_by_name() {
    let f = build_fixture();
    let commits = write_commits_file(&f);
    let assert = run([
        "--config-file-path",
        f.cfg_path.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "leg-a,no-target",
        "--repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        err.contains("no-target") && err.contains("target_repo_dir_path"),
        "error must name the series and the field: {err}"
    );
}

#[test]
fn a_duplicated_series_errors_by_name() {
    let f = build_fixture();
    let commits = write_commits_file(&f);
    let assert = run([
        "--config-file-path",
        f.cfg_path.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "leg-a,leg-a",
        "--repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let err = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        err.contains("leg-a") && err.contains("more than once"),
        "got: {err}"
    );
}

// with no --target-repo-dir-path the target comes from the series config; the explicit flag wins
#[test]
fn check_commit_defaults_the_target_from_the_series_config() {
    let f = build_fixture();
    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        f.cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "leg-b",
        "--repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        &f.sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("reasons");
    assert!(
        reasons
            .iter()
            .any(|r| r["kind"] == "qualified_call_undefined_on_target"),
        "config target must activate the gate: {reasons:?}"
    );

    // the explicit flag points at the source repo, where the gate flags nothing: the flag wins over the config
    let b = run([
        "--formatter",
        "json",
        "--config-file-path",
        f.cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "leg-b",
        "--repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        f.source.dir.path().to_str().unwrap(),
        &f.sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&b)).expect("envelope parses");
    // A clean verdict serializes no reasons array at all.
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !reasons
            .iter()
            .any(|r| r["kind"] == "qualified_call_undefined_on_target"),
        "explicit flag must win: {reasons:?}"
    );
}
