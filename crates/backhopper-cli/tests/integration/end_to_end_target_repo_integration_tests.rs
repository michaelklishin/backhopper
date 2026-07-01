// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end tests for `--target-repo-dir-path` cross-branch
//! backport assessment (doc 014). Builds source + target repos in
//! TempDirs and drives the CLI against them.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

fn make_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("add federation link");
    repo.tag("source-v1");
    repo
}

fn make_target_repo_without_path() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("README.md", "target branch\n");
    repo.commit("seed");
    repo
}

fn make_target_repo_with_translated_path() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_federation/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("seed");
    repo
}

fn write_config(workdir: &TempDir, repo: &GitRepoFixture, snapshot_dir: &Path) -> PathBuf {
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

[[path_translation]]
name = "fed_split"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
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

#[test]
fn missing_target_path_with_translation_emits_path_rename() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_with_translated_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    let results = data["results"]["results"].as_array().unwrap();
    let pin = &results[0];
    let verdict = &pin["verdict"];
    assert_eq!(
        verdict["verdict"], "requires_adaptation",
        "expected RequiresAdaptation, got: {verdict}"
    );
    let reasons = verdict["reasons"].as_array().unwrap();
    let has_path_rename = reasons.iter().any(|r| r["kind"] == "path_rename");
    assert!(
        has_path_rename,
        "expected path_rename reason in {reasons:?}"
    );
    let rename = reasons.iter().find(|r| r["kind"] == "path_rename").unwrap();
    assert_eq!(
        rename["target_path"],
        "deps/rabbitmq_federation/src/rabbit_federation_link.erl"
    );
    assert_eq!(rename["translation"]["name"], "fed_split");
}

#[test]
fn missing_target_path_without_translation_emits_inapplicable() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_without_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let data = &env["data"];
    let pin = &data["results"]["results"][0];
    let verdict = &pin["verdict"];
    assert_eq!(verdict["verdict"], "inapplicable");
    assert_eq!(verdict["reason"]["reason"], "paths_missing_on_target");
    let paths = verdict["reason"]["paths"].as_array().unwrap();
    assert!(!paths.is_empty());
}

/// One repo, two branches: v1 carries a `-x` pick of the candidate.
/// The probe must flag the candidate as already present via trailer
/// intersection, carrying the matched commit and its subject.
#[test]
fn already_present_via_x_pick_lands_in_diagnostics() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("base");
    repo.checkout_new_branch("v1");
    repo.checkout("main");
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> fixed.\n",
    );
    repo.commit("the fix");
    let cand = repo.head_sha();
    repo.tag("source-v1");
    repo.checkout("v1");
    repo.cherry_pick_x(&cand);
    let pick = repo.head_sha();
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
        "v1",
        &cand,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let identical = &env["data"]["diagnostics"]["already_present"]["identical"];
    assert_eq!(identical["via"], "trailer_intersection");
    assert_eq!(identical["commit"], pick.as_str());
    assert_eq!(identical["subject"], "the fix");
}

/// `--skip-already-present` suppresses the probe entirely: no
/// diagnostic, no skip note.
#[test]
fn skip_already_present_flag_suppresses_the_probe() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("base");
    repo.checkout_new_branch("v1");
    repo.checkout("main");
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> fixed.\n",
    );
    repo.commit("the fix");
    let cand = repo.head_sha();
    repo.tag("source-v1");
    repo.checkout("v1");
    repo.cherry_pick_x(&cand);
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
        "v1",
        "--skip-already-present",
        &cand,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    assert!(env["data"]["diagnostics"]["already_present"].is_null());
    assert!(env["data"]["diagnostics"]["already_present_skipped"].is_null());
}

/// `check batch` is the workflow path: the per-row diagnostics must
/// carry the already-present match exactly like single-commit check.
#[test]
fn batch_rows_carry_already_present_diagnostics() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("base");
    repo.checkout_new_branch("v1");
    repo.checkout("main");
    repo.write_file(
        "deps/rabbitmq_federation_common/src/rabbit_federation_link.erl",
        "-module(rabbit_federation_link).\n-export([status/0]).\nstatus() -> fixed.\n",
    );
    repo.commit("the fix");
    let cand = repo.head_sha();
    repo.tag("source-v1");
    repo.checkout("v1");
    repo.cherry_pick_x(&cand);
    let pick = repo.head_sha();
    repo.checkout("main");
    let cfg_path = write_config(&workdir, &repo, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let commits_file = workdir.path().join("commits.txt");
    fs::write(&commits_file, format!("{cand}\n")).unwrap();

    let a = run([
        "--formatter",
        "json",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "demo-series",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--target-ref",
        "v1",
        "--commits-file-path",
        commits_file.to_str().unwrap(),
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let row = &env["data"]["results"][0];
    let identical = &row["diagnostics"]["already_present"]["identical"];
    assert_eq!(identical["via"], "trailer_intersection");
    assert_eq!(identical["commit"], pick.as_str());
}

/// The candidate object is absent from the target object store: the
/// probe degrades to a skipped-note instead of silence.
#[test]
fn unrelated_target_repo_yields_a_skipped_note() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_without_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let skipped = &env["data"]["diagnostics"]["already_present_skipped"];
    assert_eq!(skipped["reason"], "candidate_missing_from_target_odb");
    assert_eq!(skipped["sha"], sha.as_str());
}

/// Builds one repo where v1 pins cowboy 2.13 and main pins 2.16,
/// with an `.app.src` chain mgmt → web_dispatch → cowboy. Returns
/// the repo and the SHA of a commit touching mgmt source.
fn build_dep_pin_fixture() -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::new();
    repo.write_file("rabbitmq-components.mk", "dep_cowboy = hex 2.13.0\n");
    repo.write_file(
        "deps/mgmt/src/mgmt.app.src",
        "{application, mgmt, [{applications, [kernel, web_dispatch]}]}.",
    );
    repo.write_file(
        "deps/web_dispatch/src/web_dispatch.app.src",
        "{application, web_dispatch, [{applications, [kernel, cowboy]}]}.",
    );
    repo.write_file(
        "deps/mgmt/src/rabbit_mgmt_db.erl",
        "-module(rabbit_mgmt_db).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("base");
    repo.checkout_new_branch("v1");
    repo.checkout("main");
    repo.write_file("rabbitmq-components.mk", "dep_cowboy = hex 2.16.0\n");
    repo.commit("bump cowboy");
    repo.write_file(
        "deps/mgmt/src/rabbit_mgmt_db.erl",
        "-module(rabbit_mgmt_db).\n-export([status/0]).\nstatus() -> fixed.\n",
    );
    repo.commit("mgmt fix");
    let sha = repo.head_sha();
    repo.tag("source-v1");
    repo.checkout("main");
    (repo, sha)
}

fn check_commit_json(repo: &GitRepoFixture, cfg_path: &Path, sha: &str) -> Value {
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
        "v1",
        sha,
    ]);
    serde_json::from_str(&stdout(&a)).expect("envelope parses")
}

/// The Cowboy shape: the touched app reaches the divergent dep only
/// transitively, through another app's `.app.src`.
#[test]
fn dep_pin_divergence_reaches_through_the_app_src_chain() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let (repo, sha) = build_dep_pin_fixture();
    let cfg_path = write_config(&workdir, &repo, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let env = check_commit_json(&repo, &cfg_path, &sha);
    let divergence = &env["data"]["diagnostics"]["dep_pin_divergence"];
    assert_eq!(divergence[0]["dep"], "cowboy");
    assert_eq!(divergence[0]["source"], "hex 2.16.0");
    assert_eq!(divergence[0]["target"], "hex 2.13.0");
}

/// A commit touching an app with no path to the divergent dep stays
/// clean: the advisory is per-commit, not per-round.
#[test]
fn dep_pin_divergence_is_silent_for_unrelated_apps() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let (repo, _) = build_dep_pin_fixture();
    repo.write_file(
        "deps/standalone/src/standalone.app.src",
        "{application, standalone, [{applications, [kernel]}]}.",
    );
    repo.write_file(
        "deps/standalone/src/standalone_worker.erl",
        "-module(standalone_worker).\n-export([status/0]).\nstatus() -> ok.\n",
    );
    repo.commit("standalone change");
    let sha = repo.head_sha();
    let cfg_path = write_config(&workdir, &repo, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let env = check_commit_json(&repo, &cfg_path, &sha);
    assert!(
        env["data"]["diagnostics"]["dep_pin_divergence"].is_null(),
        "unrelated app must not carry the divergence: {}",
        env["data"]["diagnostics"]
    );
}

#[test]
fn no_target_repo_dir_path_preserves_legacy_verdict() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let results = env["data"]["results"]["results"].as_array().unwrap();
    let verdict = &results[0]["verdict"];
    let reason_set: Vec<&str> = verdict
        .get("reasons")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|r| r["kind"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        !reason_set.contains(&"path_rename"),
        "no --target-repo-dir-path: path_rename should not appear in {reason_set:?}"
    );
}

#[test]
fn missing_translations_file_is_hard_error() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_source_repo();
    let target = make_target_repo_without_path();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
    let a = run([
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--path-translations-file-path",
        "/this/file/does/not/exist.toml",
        &sha,
    ])
    .failure();
    let err = String::from_utf8_lossy(&a.get_output().stderr);
    assert!(
        err.contains("path_translation") || err.contains("not found"),
        "expected error about missing translations file, got: {err}"
    );
}

// A cleanly-applying hunk references a macro the target branch never
// had: flagged pre-pick rather than at the build.
fn make_macro_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/demo/src/rabbit_oauth2_resource.erl",
        "-module(rabbit_oauth2_resource).\n-define(OAUTH2_BOOTSTRAP_PATH, \"/oauth\").\n-export([init/0]).\ninit() -> ok.\n",
    );
    repo.commit("seed with the macro defined");
    repo.tag("source-v1");
    repo.write_file(
        "deps/demo/src/rabbit_oauth2_resource.erl",
        "-module(rabbit_oauth2_resource).\n-define(OAUTH2_BOOTSTRAP_PATH, \"/oauth\").\n-export([init/0, bootstrap_path/0]).\ninit() -> ok.\nbootstrap_path() -> ?OAUTH2_BOOTSTRAP_PATH.\n",
    );
    repo.commit("reference the macro from a new function");
    repo
}

fn make_macro_target_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    // same module, but the branch never had the macro
    repo.write_file(
        "deps/demo/src/rabbit_oauth2_resource.erl",
        "-module(rabbit_oauth2_resource).\n-export([init/0]).\ninit() -> ok.\n",
    );
    repo.commit("seed without the macro");
    repo
}

#[test]
fn macro_undefined_on_target_is_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_macro_source_repo();
    let target = make_macro_target_repo();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let flagged = reasons
        .iter()
        .find(|r| r["kind"] == "macro_undefined_on_target")
        .unwrap_or_else(|| panic!("expected macro_undefined_on_target in {reasons:?}"));
    assert_eq!(flagged["macro_name"], "OAUTH2_BOOTSTRAP_PATH");
}

// A modified region the target branch diverged on: the preimage block
// is absent on target, so the pick will conflict. Flagged pre-pick.
fn make_preimage_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/demo/src/ra_log.erl",
        "-module(ra_log).\n-export([state/0]).\nstate() -> original.\n",
    );
    repo.commit("seed");
    repo.tag("source-v1");
    repo.write_file(
        "deps/demo/src/ra_log.erl",
        "-module(ra_log).\n-export([state/0]).\nstate() -> changed.\n",
    );
    repo.commit("change state");
    repo
}

fn make_preimage_target_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/demo/src/ra_log.erl",
        "-module(ra_log).\n-export([state/0]).\nstate() -> diverged.\n",
    );
    repo.commit("seed diverged");
    repo
}

#[test]
fn target_preimage_divergence_is_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = make_preimage_source_repo();
    let target = make_preimage_target_repo();
    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);

    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    assert!(
        reasons.iter().any(|r| r["kind"] == "preimage_missing"),
        "expected preimage_missing in {reasons:?}"
    );
}

// A `#record{}` use whose definition the target branch lacks.
#[test]
fn record_undefined_on_target_is_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/ra_machine.erl",
        "-module(ra_machine).\n-record(cfg, {field}).\n-export([init/1]).\ninit(S) -> S.\n",
    );
    source.commit("seed with the record");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/ra_machine.erl",
        "-module(ra_machine).\n-record(cfg, {field}).\n-export([init/1]).\ninit(S) -> S#cfg.field.\n",
    );
    source.commit("use the record");
    let target = GitRepoFixture::new();
    target.write_file(
        "deps/demo/src/ra_machine.erl",
        "-module(ra_machine).\n-export([init/1]).\ninit(S) -> S.\n",
    );
    target.commit("seed without the record");

    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let r = reasons
        .iter()
        .find(|r| r["kind"] == "record_undefined_on_target")
        .unwrap_or_else(|| panic!("expected record_undefined_on_target in {reasons:?}"));
    assert_eq!(r["record_name"], "cfg");
}

// A local call to a function a sibling commit added that the target lacks.
#[test]
fn local_call_undefined_on_target_is_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/w.erl",
        "-module(w).\n-export([g/0]).\nhelper() -> ok.\ng() -> ok.\n",
    );
    source.commit("seed with helper");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/w.erl",
        "-module(w).\n-export([g/0]).\nhelper() -> ok.\ng() -> helper().\n",
    );
    source.commit("call helper");
    let target = GitRepoFixture::new();
    target.write_file(
        "deps/demo/src/w.erl",
        "-module(w).\n-export([g/0]).\ng() -> ok.\n",
    );
    target.commit("seed without helper");

    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let r = reasons
        .iter()
        .find(|r| r["kind"] == "local_call_undefined_on_target")
        .unwrap_or_else(|| panic!("expected local_call_undefined_on_target in {reasons:?}"));
    assert_eq!(r["function"], "helper");
    // The call is the fourth line of the file: the line map made the
    // report file-relative for the existing reasons too.
    assert_eq!(r["line"], 4);
}

// A patch that adds both a qualified call and the callee (with its
// export, in the callee's own file) must not false-positive: the
// collector's patch-wide added-functions map covers the cross-file case
// even though the target tree lacks the new function.
#[test]
fn a_cross_file_patch_added_callee_is_not_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    source.commit("seed caller");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\nf() -> helper:fresh(x).\n",
    );
    source.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([fresh/1]).\nfresh(X) -> X.\n",
    );
    source.commit("add helper:fresh/1 and call it");
    let target = GitRepoFixture::new();
    target.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    target.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([present/0]).\npresent() -> ok.\n",
    );
    target.commit("seed caller and helper without fresh/1");

    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    assert!(
        !reasons
            .iter()
            .any(|r| r["kind"] == "qualified_call_undefined_on_target"),
        "patch-added callee should not be flagged: {reasons:?}"
    );
}

// A patch that adds a qualified call into a first-party module the
// target tree does not export is flagged, and the reported line is the
// true file line (the #16771 shape). `helper` is present on the target
// tree but absent from the demo snapshot, so it resolves live.
#[test]
fn qualified_call_undefined_on_target_is_flagged_through_the_cli() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    source.commit("seed caller");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\nf() -> helper:missing(x).\n",
    );
    source.commit("call helper:missing/1");
    let target = GitRepoFixture::new();
    target.write_file(
        "deps/demo/src/caller.erl",
        "-module(caller).\n-export([go/0]).\ngo() -> ok.\n",
    );
    target.write_file(
        "deps/demo/src/helper.erl",
        "-module(helper).\n-export([present/0]).\npresent() -> ok.\n",
    );
    target.commit("seed caller and helper without missing/1");

    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let r = reasons
        .iter()
        .find(|r| r["kind"] == "qualified_call_undefined_on_target")
        .unwrap_or_else(|| panic!("expected qualified_call_undefined_on_target in {reasons:?}"));
    assert_eq!(r["module"], "helper");
    assert_eq!(r["function"], "missing");
    assert_eq!(r["arity"], 1);
    // The call is the fourth line of the new file, not the first line
    // of the added block: the line map made the report file-relative.
    assert_eq!(r["line"], 4);

    // The text table names the reason and the m:f/a, not "UnknownReason".
    let t = run([
        "--formatter",
        "text",
        "--config-file-path",
        cfg_path.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "demo-series",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let text = stdout(&t);
    assert!(
        text.contains("QualifiedCallUndefinedOnTarget") && text.contains("helper:missing/1"),
        "text table missing the reason: {text}"
    );
}

// A patch touching one present and one absent target path: the absent
// path surfaces as a TargetPathAbsent reason.
#[test]
fn partial_target_absence_emits_target_path_absent() {
    let workdir = TempDir::new().unwrap();
    let snapshot_dir = workdir.path().join("snap");
    // mgmt.erl exists on the source pin but not the target tree.
    let source = GitRepoFixture::new();
    source.write_file(
        "deps/demo/src/core.erl",
        "-module(core).\n-export([go/0]).\ngo() -> ok.\n",
    );
    source.write_file(
        "deps/demo/src/mgmt.erl",
        "-module(mgmt).\n-export([m/0]).\nm() -> ok.\n",
    );
    source.commit("seed core and mgmt");
    source.tag("source-v1");
    source.write_file(
        "deps/demo/src/core.erl",
        "-module(core).\n-export([go/0]).\ngo() -> {ok}.\n",
    );
    source.write_file(
        "deps/demo/src/mgmt.erl",
        "-module(mgmt).\n-export([m/0]).\nm() -> core:go().\n",
    );
    source.commit("edit both bodies");
    let target = GitRepoFixture::new();
    target.write_file(
        "deps/demo/src/core.erl",
        "-module(core).\n-export([go/0]).\ngo() -> ok.\n",
    );
    target.commit("seed without mgmt");

    let cfg_path = write_config(&workdir, &source, &snapshot_dir);
    generate_snapshot(&cfg_path);
    let sha = source.head_sha();
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
        source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &sha,
    ]);
    let env: Value = serde_json::from_str(&stdout(&a)).expect("envelope parses");
    let reasons = env["data"]["results"]["results"][0]["verdict"]["reasons"]
        .as_array()
        .expect("a reasons array");
    let r = reasons
        .iter()
        .find(|r| r["kind"] == "target_path_absent")
        .unwrap_or_else(|| panic!("expected target_path_absent in {reasons:?}"));
    assert_eq!(r["path"], "deps/demo/src/mgmt.erl");
}
