// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end coverage of the apply forecast: a suite-only candidate
//! whose target diverged must surface its predicted conflict through
//! the row, the clearance, and the exit code even though every pin
//! verdict is inapplicable.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

use crate::helpers::cli::{run, run_succeeds, stdout};
use backhopper_test_support::{GitRepoFixture, toml_path};

const SRC_MODULE: &str =
    "-module(rabbit_fifo).\n-export([apply/3]).\napply(_, _, State) -> State.\n";

const SUITE_V1: &str = "-module(quorum_queue_SUITE).\n-export([all/0]).\nall() -> [t_declare].\n";

const SUITE_V2: &str =
    "-module(quorum_queue_SUITE).\n-export([all/0]).\nall() -> [t_declare, t_grow].\n";

const SUITE_DIVERGED: &str =
    "-module(quorum_queue_SUITE).\n-export([groups/0]).\ngroups() -> [{parallel, []}].\n";

fn write_config(dir: &Path, repo: &Path, snapshot_dir: &Path) -> PathBuf {
    fs::create_dir_all(snapshot_dir).unwrap();
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

/// Source repo: a tagged baseline, then a suite-only candidate commit.
fn build_source_repo() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    repo.write_file("test/quorum_queue_SUITE.erl", SUITE_V1);
    repo.commit("baseline");
    repo.tag("v1.0.0");
    repo.write_file("test/quorum_queue_SUITE.erl", SUITE_V2);
    repo.commit("extend the suite");
    repo
}

/// Target where the suite exists but its content diverged from the
/// candidate's preimage.
fn build_diverged_target() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    repo.write_file("test/quorum_queue_SUITE.erl", SUITE_DIVERGED);
    repo.commit("diverged target");
    repo
}

/// Target that never shipped the suite at all.
fn build_target_without_suite() -> GitRepoFixture {
    let repo = GitRepoFixture::new();
    repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
    repo.commit("target without the suite");
    repo
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

struct Round {
    workdir: TempDir,
    cfg: PathBuf,
    source: GitRepoFixture,
    sha: String,
}

fn set_up_round() -> Round {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let cfg = write_config(
        workdir.path(),
        source.dir.path(),
        &workdir.path().join("snapshots"),
    );
    generate_snapshots(&cfg);
    let sha = source.head_sha();
    Round {
        workdir,
        cfg,
        source,
        sha,
    }
}

fn check_commit_json(round: &Round, target: Option<&Path>) -> (Value, i32) {
    let mut args: Vec<String> = vec![
        "--formatter".into(),
        "json".into(),
        "--config-file-path".into(),
        round.cfg.to_str().unwrap().into(),
        "check".into(),
        "commit".into(),
        "--series".into(),
        "stable".into(),
        "--repo-dir-path".into(),
        round.source.dir.path().to_str().unwrap().into(),
    ];
    if let Some(target) = target {
        args.push("--target-repo-dir-path".into());
        args.push(target.to_str().unwrap().into());
    }
    args.push(round.sha.clone());
    let assert = run(args);
    let code = assert.get_output().status.code().unwrap();
    let env: Value = serde_json::from_str(&stdout(&assert)).expect("envelope parses");
    (env, code)
}

#[test]
fn suite_only_conflict_survives_an_inapplicable_verdict() {
    let round = set_up_round();
    let target = build_diverged_target();
    let (env, code) = check_commit_json(&round, Some(target.dir.path()));
    let data = &env["data"];
    let pin = &data["results"]["results"][0];
    assert_eq!(
        pin["verdict"]["verdict"], "inapplicable",
        "suite-only diff should stay inapplicable, got: {}",
        pin["verdict"]
    );
    let outcome = &data["apply"]["paths"]["test/quorum_queue_SUITE.erl"];
    assert_eq!(
        outcome["outcome"], "conflict",
        "expected a conflict: {data}"
    );
    assert_eq!(outcome["kind"], "preimage_missing");
    assert_eq!(code, 3, "a predicted conflict must exit NEEDS_ATTENTION");
    assert_eq!(env["exit_code"], 3);
}

#[test]
fn without_a_target_context_the_forecast_is_absent_and_exit_is_zero() {
    let round = set_up_round();
    let (env, code) = check_commit_json(&round, None);
    assert!(
        env["data"].get("apply").is_none(),
        "no target context, so the apply axis must be absent: {}",
        env["data"]
    );
    assert_eq!(code, 0);
}

#[test]
fn a_path_absent_on_target_forecasts_file_absent() {
    let round = set_up_round();
    let target = build_target_without_suite();
    let (env, code) = check_commit_json(&round, Some(target.dir.path()));
    let outcome = &env["data"]["apply"]["paths"]["test/quorum_queue_SUITE.erl"];
    assert_eq!(outcome["outcome"], "conflict");
    assert_eq!(outcome["kind"], "file_absent");
    assert_eq!(code, 3);
}

#[test]
fn batch_folds_the_forecast_into_clearance_text_and_exit() {
    let round = set_up_round();
    let target = build_diverged_target();
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}", round.sha).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("apply forecast      : 1 row of 1 conflicts on 1 path (preimage_missing=1)"),
        "clearance must state the conflict, got:\n{out}"
    );
    assert!(
        out.contains("apply conflicts: test/quorum_queue_SUITE.erl (preimage_missing)"),
        "the row must name the conflicting path, got:\n{out}"
    );
    assert_eq!(code, 3);
    let _ = &round.workdir;
}

#[test]
fn batch_with_a_clean_target_stays_zero_domain_with_forecast_coverage() {
    let round = set_up_round();
    // The target matches the source baseline, so every hunk applies.
    let target = {
        let repo = GitRepoFixture::new();
        repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
        repo.write_file("test/quorum_queue_SUITE.erl", SUITE_V1);
        repo.commit("target at baseline");
        repo
    };
    let mut commits = NamedTempFile::new().unwrap();
    writeln!(commits, "{}", round.sha).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.path().to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("apply forecast      : clean across 1 row"),
        "clean forecast must be stated, got:\n{out}"
    );
    assert!(
        out.contains("no apply conflicts were predicted"),
        "zero-domain narrative must claim the apply axis, got:\n{out}"
    );
    assert_eq!(code, 0);
}

#[test]
fn cascade_marks_the_conflicting_leg_in_the_matrix() {
    let workdir = TempDir::new().unwrap();
    let source = build_source_repo();
    let clean_target = {
        let repo = GitRepoFixture::new();
        repo.write_file("src/rabbit_fifo.erl", SRC_MODULE);
        repo.write_file("test/quorum_queue_SUITE.erl", SUITE_V1);
        repo.commit("target at baseline");
        repo
    };
    let diverged_target = build_diverged_target();
    let snapshot_dir = workdir.path().join("snapshots");
    fs::create_dir_all(&snapshot_dir).unwrap();
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

[[series]]
name = "clean-leg"
pins = [ {{ project = "demo", tag = "v1.0.0" }} ]
target_repo_dir_path = "{}"

[[series]]
name = "conflicted-leg"
pins = [ {{ project = "demo", tag = "v1.0.0" }} ]
target_repo_dir_path = "{}"
"#,
        toml_path(&snapshot_dir),
        toml_path(source.dir.path()),
        toml_path(clean_target.dir.path()),
        toml_path(diverged_target.dir.path()),
    );
    let cfg = workdir.path().join("backhopper.toml");
    fs::write(&cfg, body).unwrap();
    generate_snapshots(&cfg);
    let commits = workdir.path().join("commits.txt");
    fs::write(&commits, format!("{}\n", source.head_sha())).unwrap();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        cfg.to_str().unwrap(),
        "check",
        "cascade",
        "--series",
        "clean-leg,conflicted-leg",
        "--repo-dir-path",
        source.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("inapplicable*"),
        "the conflicting leg's cell must carry the marker, got:\n{out}"
    );
    assert!(
        out.contains("* apply conflict predicted on this leg"),
        "the legend must explain the marker, got:\n{out}"
    );
    assert_eq!(code, 3, "the conflicted leg must drive the cascade exit");
    let _ = (&clean_target, &diverged_target);
}

#[test]
fn single_check_text_output_names_the_conflict() {
    let round = set_up_round();
    let target = build_diverged_target();
    let assert = run([
        "--formatter",
        "text",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &round.sha,
    ]);
    let code = assert.get_output().status.code().unwrap();
    let out = stdout(&assert);
    assert!(
        out.contains("apply conflict: test/quorum_queue_SUITE.erl (preimage_missing)"),
        "text output must explain the non-zero exit, got:\n{out}"
    );
    assert_eq!(code, 3);
}

#[test]
fn text_summary_counts_the_conflicting_paths() {
    let round = set_up_round();
    let target = build_diverged_target();
    let assert = run([
        "--formatter",
        "text-summary",
        "--config-file-path",
        round.cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        round.source.dir.path().to_str().unwrap(),
        "--target-repo-dir-path",
        target.dir.path().to_str().unwrap(),
        &round.sha,
    ]);
    let out = stdout(&assert);
    let fields: Vec<&str> = out.lines().next().expect("one row").split('\t').collect();
    assert_eq!(fields[1], "inapplicable");
    // sixth column is the apply-conflict count
    assert_eq!(fields[5], "1");
}
