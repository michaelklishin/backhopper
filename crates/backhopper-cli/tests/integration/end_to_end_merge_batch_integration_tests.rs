// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Merge-aware `check batch`: per-row parent
//! counts, verdict parity with `check merge`, summary projections,
//! up-front line validation, and `--terse` rejection.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, run_fails, run_succeeds, stderr, stdout};
use backhopper_test_support::GitRepoFixture;

const ERL_BASE: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> Name.
"#;

const ERL_PLAIN: &str = r#"
-module(demo_mod).
-export([greet/1]).
greet(Name) -> demo_mod:greet(Name).
"#;

const ERL_FEATURE: &str = r#"
-module(demo_mod).
-export([greet/1, greet/2]).
greet(Name) -> Name.
greet(_, _) -> demo_mod:greet(<<"x">>).
"#;

struct MergeFixture {
    repo: GitRepoFixture,
    workdir: TempDir,
    cfg: PathBuf,
    plain_sha: String,
    merge_sha: String,
}

/// Repo shape: tagged base, one plain commit on `main`, then a
/// `--no-ff` merge of a one-commit feature branch.
fn merge_fixture() -> MergeFixture {
    let workdir = TempDir::new().unwrap();
    let repo = GitRepoFixture::new();
    repo.write_file("src/demo_mod.erl", ERL_BASE);
    repo.commit("base");
    repo.tag("v1.0.0");
    repo.write_file("src/demo_mod.erl", ERL_PLAIN);
    repo.commit("plain change");
    let plain_sha = repo.head_sha();
    repo.checkout_new_branch("feature");
    repo.write_file("src/demo_mod.erl", ERL_FEATURE);
    repo.commit("feature work");
    repo.checkout("main");
    repo.merge_no_ff("feature", "Merge branch 'feature'");
    let merge_sha = repo.head_sha();

    let snapshot_dir = workdir.path().join("snapshots");
    let cfg = write_series_config(workdir.path(), repo.dir.path(), &snapshot_dir);
    run_succeeds([
        "--config-file-path",
        cfg.to_str().unwrap(),
        "snapshots",
        "generate",
        "--project",
        "demo",
    ]);
    MergeFixture {
        repo,
        workdir,
        cfg,
        plain_sha,
        merge_sha,
    }
}

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
        snapshot_dir.display(),
        repo.display(),
    );
    let p = dir.join("backhopper.toml");
    fs::write(&p, body).unwrap();
    p
}

fn write_commits_file(fixture: &MergeFixture, lines: &[&str]) -> PathBuf {
    let p = fixture.workdir.path().join("commits.txt");
    fs::write(&p, lines.join("\n")).unwrap();
    p
}

fn batch_json(fixture: &MergeFixture, commits: &Path) -> Value {
    let assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    serde_json::from_str(&stdout(&assert)).expect("batch envelope is JSON")
}

#[test]
fn batch_accepts_merge_shas_and_reports_parent_counts() {
    let fixture = merge_fixture();
    let commits = write_commits_file(&fixture, &[&fixture.plain_sha, &fixture.merge_sha]);
    let env = batch_json(&fixture, &commits);
    let results = env["data"]["results"].as_array().expect("results array");
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0]["commit"],
        Value::String(fixture.plain_sha.clone())
    );
    assert_eq!(results[0]["parent_count"], Value::from(1));
    assert!(results[0]["pr_commits"].is_null());
    assert_eq!(
        results[1]["commit"],
        Value::String(fixture.merge_sha.clone())
    );
    assert_eq!(results[1]["parent_count"], Value::from(2));
    let pr_commits = results[1]["pr_commits"]
        .as_array()
        .expect("2-parent merge row carries pr_commits");
    assert_eq!(pr_commits.len(), 1);
    assert_eq!(pr_commits[0]["subject"], "feature work");
}

#[test]
fn batch_merge_row_verdict_matches_check_merge() {
    let fixture = merge_fixture();
    let commits = write_commits_file(&fixture, &[&fixture.merge_sha]);
    let batch_env = batch_json(&fixture, &commits);
    let batch_row = &batch_env["data"]["results"][0];

    let merge_assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "json",
        "check",
        "merge",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        &fixture.merge_sha,
    ]);
    let merge_env: Value =
        serde_json::from_str(&stdout(&merge_assert)).expect("merge envelope is JSON");

    assert_eq!(
        batch_row["verdict"]["summary"], merge_env["data"]["results"]["summary"],
        "batch merge row and check merge must agree on the verdict summary"
    );
    assert_eq!(
        batch_row["pr_commits"], merge_env["data"]["pr_commits"],
        "batch merge row and check merge must agree on pr_commits"
    );
    assert_eq!(batch_env["exit_code"], merge_env["exit_code"]);
}

#[test]
fn batch_summary_formatter_emits_one_jsonl_row_per_pair() {
    let fixture = merge_fixture();
    let commits = write_commits_file(&fixture, &[&fixture.plain_sha, &fixture.merge_sha]);
    let assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "summary",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let out = stdout(&assert);
    let rows: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is a JSON row"))
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["sha"], Value::String(fixture.plain_sha.clone()));
    assert_eq!(rows[0]["series"], "stable");
    assert_eq!(rows[0]["parent_count"], Value::from(1));
    assert_eq!(rows[0]["subject"], "plain change");
    assert_eq!(rows[1]["sha"], Value::String(fixture.merge_sha.clone()));
    assert_eq!(rows[1]["parent_count"], Value::from(2));
    assert_eq!(rows[1]["subject"], "Merge branch 'feature'");
}

#[test]
fn batch_text_summary_carries_series_column_before_subject() {
    let fixture = merge_fixture();
    let commits = write_commits_file(&fixture, &[&fixture.plain_sha]);
    let assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "text-summary",
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let out = stdout(&assert);
    let line = out.lines().next().expect("one row");
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(
        fields.len(),
        6,
        "sha, verdict, touched, tracked, series, subject"
    );
    assert_eq!(fields[4], "stable");
    assert_eq!(fields[5], "plain change");
}

#[test]
fn batch_rejects_terse_with_a_summary_hint() {
    let fixture = merge_fixture();
    let commits = write_commits_file(&fixture, &[&fixture.plain_sha]);
    let assert = run_fails([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
        "--terse",
    ]);
    let err = stderr(&assert);
    assert!(
        err.contains("--terse is not supported on `check batch`"),
        "{err}"
    );
    assert!(err.contains("--formatter summary"), "{err}");
}

#[test]
fn batch_reports_every_bad_line_before_evaluating_anything() {
    let fixture = merge_fixture();
    let root_sha = fixture.repo.root_sha();
    let commits = write_commits_file(
        &fixture,
        &[&fixture.plain_sha, "deadbeefdeadbeef", &root_sha],
    );
    let assert = run_fails([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        "--commits-file-path",
        commits.to_str().unwrap(),
    ]);
    let err = stderr(&assert);
    assert!(
        err.contains("2 commits-file line(s) failed to resolve"),
        "{err}"
    );
    assert!(err.contains("line 2:"), "{err}");
    assert!(err.contains("line 3:"), "{err}");
    assert!(err.contains("has no parent"), "{err}");
}

#[test]
fn check_commit_still_refuses_merge_shas_with_the_redirect() {
    let fixture = merge_fixture();
    let assert = run_fails([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        &fixture.merge_sha,
    ]);
    let err = stderr(&assert);
    assert!(
        err.contains(&format!(
            "{} is a merge commit (2 parents); use 'backhopper check merge {}' instead",
            fixture.merge_sha, fixture.merge_sha
        )),
        "{err}"
    );
}

#[test]
fn check_commit_summary_row_carries_the_real_sha_series_and_parent_count() {
    let fixture = merge_fixture();
    let assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "summary",
        "check",
        "commit",
        "--series",
        "stable",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        &fixture.plain_sha,
    ]);
    let out = stdout(&assert);
    let row: Value = serde_json::from_str(out.lines().next().unwrap()).expect("JSONL row");
    assert_eq!(row["sha"], Value::String(fixture.plain_sha.clone()));
    assert_eq!(row["series"], "stable");
    assert_eq!(row["parent_count"], Value::from(1));
    assert_eq!(row["subject"], "plain change");
}

#[test]
fn ours_merge_row_is_inapplicable_with_parent_count_two() {
    let fixture = merge_fixture();
    fixture.repo.checkout_new_branch("dropped");
    fixture
        .repo
        .write_file("src/dropped.erl", "-module(dropped).\n");
    fixture.repo.commit("dropped work");
    fixture.repo.checkout("main");
    fixture
        .repo
        .merge_ours("dropped", "Merge branch 'dropped' (dropped)");
    let ours_sha = fixture.repo.head_sha();
    let commits = write_commits_file(&fixture, &[&ours_sha]);
    let env = batch_json(&fixture, &commits);
    let row = &env["data"]["results"][0];
    assert_eq!(row["parent_count"], Value::from(2));
    let summary = &row["verdict"]["summary"];
    assert_eq!(summary["compatible"], 0);
    assert_eq!(summary["incompatible"], 0);
    assert_eq!(summary["requires_adaptation"], 0);
    assert_eq!(summary["inapplicable"], 1);
}

#[test]
fn pin_targeted_text_summary_renders_a_dash_for_the_series_column() {
    let fixture = merge_fixture();
    let assert = run([
        "--config-file-path",
        fixture.cfg.to_str().unwrap(),
        "--formatter",
        "text-summary",
        "check",
        "commit",
        "--project",
        "demo",
        "--tag",
        "v1.0.0",
        "--repo-dir-path",
        fixture.repo.dir.path().to_str().unwrap(),
        &fixture.plain_sha,
    ]);
    let out = stdout(&assert);
    let fields: Vec<&str> = out.lines().next().expect("one row").split('\t').collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(fields[4], "-");
    assert_eq!(fields[5], "plain change");
}
