// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end coverage of `siblings doctor` through the process
//! boundary: a two-branch repo where one fix cascaded (suppressed)
//! and one did not (surfaces).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

use crate::helpers::cli::{run, stderr, stdout};

/// A workspace with a config, a snapshot dir, and a two-branch repo:
///
/// * `main`: base, an unswept flake fix (surfaces), a `-x`-picked
///   crash-gate fix (suppressed), and a non-matching feature commit
/// * `v1.x`: forked from base, tagged `v1.0.0`, then receives the
///   cherry-pick
struct DoctorFixture {
    dir: TempDir,
    unswept_sha: String,
    picked_sha: String,
}

impl DoctorFixture {
    fn new(family: &str) -> Self {
        let dir = TempDir::new().expect("tempdir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);

        write(
            &repo,
            "src/app.erl",
            "-module(app).\n-export([f/0]).\nf() -> ok.\n",
        );
        write(
            &repo,
            "test/app_SUITE.erl",
            "-module(app_SUITE).\n-export([t/1]).\nt(_) -> ok.\n",
        );
        git(&repo, &["add", "-A"]);
        commit(&repo, "init", "2025-06-01T10:00:00Z");

        git(&repo, &["checkout", "-q", "-b", "v1.x"]);
        commit_empty(&repo, "release prep", "2025-07-01T10:00:00Z");
        git(&repo, &["tag", "v1.0.0"]);
        git(&repo, &["checkout", "-q", "main"]);

        // surfaces: small flake fix, never cherry-picked
        write(
            &repo,
            "test/app_SUITE.erl",
            "-module(app_SUITE).\n-export([t/1]).\nt(_) -> wait_for(ok).\n",
        );
        git(&repo, &["add", "-A"]);
        commit(
            &repo,
            "Deflake app_SUITE: wait_for the condition",
            "2025-08-01T10:00:00Z",
        );
        let unswept_sha = head(&repo);

        // suppressed: crash-gate fix cherry-picked to v1.x with -x
        write(
            &repo,
            "test/other_SUITE.erl",
            "-module(other_SUITE).\n-export([t/1]).\nt(_) -> no_crash.\n",
        );
        git(&repo, &["add", "-A"]);
        commit(
            &repo,
            "Don't find crashes in logs in other_SUITE",
            "2025-08-02T10:00:00Z",
        );
        let picked_sha = head(&repo);

        // walked but never a candidate: no vocabulary match
        write(
            &repo,
            "src/app.erl",
            "-module(app).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n",
        );
        git(&repo, &["add", "-A"]);
        commit(&repo, "Add g/0", "2025-08-03T10:00:00Z");

        git(&repo, &["checkout", "-q", "v1.x"]);
        git_dated(
            &repo,
            &["cherry-pick", "-x", &picked_sha],
            "2025-08-04T10:00:00Z",
        );
        git(&repo, &["checkout", "-q", "main"]);

        let config = format!(
            r#"
config_version = 1

[defaults]
snapshot_dir    = "snapshots"
fallback_branch = "main"

[[project]]
name        = "demo"
kind        = "self"
family      = "{family}"
tag_pattern = "v*"

[[series]]
name = "demo-1.0"
pins = [
  {{ project = "demo", branch = "v1.x" }},
]
"#
        );
        std::fs::write(dir.path().join("backhopper.toml"), config).unwrap();
        std::fs::create_dir_all(dir.path().join("snapshots")).unwrap();
        Self {
            dir,
            unswept_sha,
            picked_sha,
        }
    }

    fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    fn config(&self) -> String {
        self.dir
            .path()
            .join("backhopper.toml")
            .to_string_lossy()
            .into_owned()
    }

    fn doctor_args(&self) -> Vec<String> {
        vec![
            "--config-file-path".into(),
            self.config(),
            "--formatter".into(),
            "json".into(),
            "siblings".into(),
            "doctor".into(),
            "--series".into(),
            "demo-1.0".into(),
            "--repo-dir-path".into(),
            self.repo().to_string_lossy().into_owned(),
        ]
    }
}

fn git(repo: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e");
    let status = cmd.status().expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn git_dated(repo: &Path, args: &[&str], date: &str) {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date);
    let status = cmd.status().expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn commit(repo: &Path, message: &str, date: &str) {
    git_dated(repo, &["commit", "-q", "-m", message], date);
}

fn commit_empty(repo: &Path, message: &str, date: &str) {
    git_dated(
        repo,
        &["commit", "-q", "--allow-empty", "-m", message],
        date,
    );
}

fn write(repo: &Path, rel: &str, body: &str) {
    let path = repo.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn head(repo: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn parse_envelope(raw: &str) -> Value {
    serde_json::from_str(raw).expect("stdout is a JSON envelope")
}

#[test]
fn unswept_fix_surfaces_and_x_picked_fix_is_suppressed() {
    let fx = DoctorFixture::new("rabbitmq");
    let assert = run(fx.doctor_args()).code(3);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["schema_version"], 8);
    assert_eq!(body["command"], "siblings doctor");
    let data = &body["data"];
    assert_eq!(data["series"], "demo-1.0");
    assert_eq!(data["target_branch"], "v1.x");
    assert_eq!(data["since"]["kind"], "last_release_tag");
    assert_eq!(data["since"]["tag"], "v1.0.0");
    assert_eq!(data["vocabulary_source"], "family_default");
    assert_eq!(data["suppressed_count"], 1);
    assert_eq!(data["walked_count"], 3);
    let candidates = data["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    assert!(
        candidates
            .iter()
            .all(|c| c["sha"] != fx.picked_sha.as_str()),
        "the cascaded fix must stay suppressed"
    );
    let row = &candidates[0];
    assert_eq!(row["sha"], fx.unswept_sha.as_str());
    assert_eq!(row["subject"], "Deflake app_SUITE: wait_for the condition");
    assert_eq!(row["action"], "sweep");
    assert_eq!(row["parent_count"], 1);
    assert_eq!(row["touched_paths"][0], "test/app_SUITE.erl");
    // not --explain, so the breakdown is stripped
    assert!(row.get("score_components").is_none());
}

#[test]
fn merge_commits_surface_once_under_the_pr_title() {
    let fx = DoctorFixture::new("rabbitmq");
    let repo = fx.repo();
    // a PR branch merged with GitHub's boilerplate subject; the PR
    // title rides the body
    git(&repo, &["checkout", "-q", "-b", "pr"]);
    write(
        &repo,
        "test/app_SUITE.erl",
        "-module(app_SUITE).\n-export([t/1]).\nt(_) -> wait_for(ok), retry.\n",
    );
    git(&repo, &["add", "-A"]);
    commit(
        &repo,
        "inner commit, subject irrelevant",
        "2025-08-05T10:00:00Z",
    );
    git(&repo, &["checkout", "-q", "main"]);
    git_dated(
        &repo,
        &[
            "merge",
            "--no-ff",
            "-m",
            "Merge pull request #42 from org/pr\n\nFix an eventually-consistent flake in app_SUITE",
            "pr",
        ],
        "2025-08-06T10:00:00Z",
    );
    let merge_sha = head(&repo);

    let assert = run(fx.doctor_args()).code(3);
    let body = parse_envelope(&stdout(&assert));
    let candidates = body["data"]["candidates"].as_array().unwrap();
    let merge_row = candidates
        .iter()
        .find(|c| c["sha"] == merge_sha.as_str())
        .expect("the merge SHA surfaces as one candidate");
    assert_eq!(merge_row["parent_count"], 2);
    assert_eq!(
        merge_row["subject"],
        "Fix an eventually-consistent flake in app_SUITE"
    );
    // the inner commit must not surface separately
    assert!(candidates.iter().all(|c| c["parent_count"] != 0));
    assert_eq!(candidates.len(), 2);
}

#[test]
fn hand_landed_pick_without_a_trailer_is_suppressed_by_patch_id() {
    let fx = DoctorFixture::new("rabbitmq");
    let repo = fx.repo();
    // pick the unswept fix WITHOUT -x: no trailer, same content
    git(&repo, &["checkout", "-q", "v1.x"]);
    git_dated(
        &repo,
        &["cherry-pick", &fx.unswept_sha],
        "2025-08-07T10:00:00Z",
    );
    git(&repo, &["checkout", "-q", "main"]);

    let assert = run(fx.doctor_args()).code(0);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["suppressed_count"], 2);
    assert!(body["data"]["candidates"].as_array().unwrap().is_empty());
}

#[test]
fn generic_family_reports_empty_vocabulary_and_exits_zero() {
    let fx = DoctorFixture::new("generic");
    let assert = run(fx.doctor_args()).code(0);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["vocabulary_source"], "empty");
    assert!(body["data"]["candidates"].as_array().unwrap().is_empty());
    // the window is still counted so the report stays meaningful
    assert_eq!(body["data"]["walked_count"], 3);
    assert_eq!(body["data"]["suppressed_count"], 0);
}

#[test]
fn series_without_self_pin_requires_target_branch() {
    let fx = DoctorFixture::new("rabbitmq");
    let config = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name        = "demo"
kind        = "self"
family      = "rabbitmq"
tag_pattern = "v*"

[[project]]
name    = "dep"
git_url = "{}"

[[series]]
name = "demo-1.0"
pins = [
  {{ project = "dep", tag = "v1.0.0" }},
]
"#,
        fx.repo().display()
    );
    std::fs::write(fx.dir.path().join("backhopper.toml"), config).unwrap();
    let assert = run(fx.doctor_args()).code(64);
    let err = stderr(&assert);
    assert!(err.contains("has no self-pin"), "stderr: {err}");
    assert!(err.contains("--target-branch"), "stderr: {err}");

    // the override unblocks the same series
    let mut args = fx.doctor_args();
    args.push("--target-branch".into());
    args.push("v1.x".into());
    run(args).code(3);
}

#[test]
fn no_reachable_release_tag_points_at_since() {
    let fx = DoctorFixture::new("rabbitmq");
    let repo = fx.repo();
    // strip the only reachable tag and put a newer one on main, which
    // the target branch cannot reach
    git(&repo, &["tag", "-d", "v1.0.0"]);
    git(&repo, &["tag", "v9.9.9", "main"]);
    let assert = run(fx.doctor_args()).code(64);
    let err = stderr(&assert);
    assert!(err.contains("no release tag"), "stderr: {err}");
    assert!(err.contains("--since"), "stderr: {err}");
}

#[test]
fn explicit_since_forms_round_trip_the_derivation() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut tag_args = fx.doctor_args();
    tag_args.push("--since".into());
    tag_args.push("v1.0.0".into());
    let assert = run(tag_args).code(3);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["since"]["kind"], "explicit_tag");
    assert_eq!(body["data"]["since"]["tag"], "v1.0.0");

    // a 12-hex prefix expands to the full SHA; the window past it
    // holds only suppressed or non-matching commits, hence exit 0
    let mut sha_args = fx.doctor_args();
    sha_args.push("--since".into());
    sha_args.push(fx.unswept_sha[..12].to_owned());
    let assert = run(sha_args).code(0);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["since"]["kind"], "explicit_sha");
    assert_eq!(body["data"]["since"]["sha"], fx.unswept_sha.as_str());
    assert_eq!(body["data"]["walked_count"], 2);
}

#[test]
fn since_equal_to_the_source_tip_yields_an_empty_window() {
    let fx = DoctorFixture::new("rabbitmq");
    let tip = head(&fx.repo());
    let mut args = fx.doctor_args();
    args.push("--since".into());
    args.push(tip);
    let assert = run(args).code(0);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["walked_count"], 0);
    assert!(body["data"]["candidates"].as_array().unwrap().is_empty());
}

#[test]
fn top_truncates_the_ranked_list() {
    let fx = DoctorFixture::new("rabbitmq");
    let repo = fx.repo();
    // a second unswept candidate
    write(
        &repo,
        "test/app_SUITE.erl",
        "-module(app_SUITE).\n-export([t/1]).\nt(_) -> wait_for(ok), ok.\n",
    );
    git(&repo, &["add", "-A"]);
    commit(
        &repo,
        "Fix a flaky retry in app_SUITE",
        "2025-08-08T10:00:00Z",
    );

    let mut args = fx.doctor_args();
    args.push("--top".into());
    args.push("1".into());
    let assert = run(args).code(3);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["candidates"].as_array().unwrap().len(), 1);
}

#[test]
fn explain_includes_the_score_breakdown() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    args.push("--explain".into());
    let assert = run(args).code(3);
    let body = parse_envelope(&stdout(&assert));
    let row = &body["data"]["candidates"][0];
    let components = &row["score_components"];
    assert!(components["line_count_factor"].as_f64().unwrap() > 0.0);
    // deflake, wait_for, and _SUITE all hit
    assert_eq!(components["vocabulary_terms_matched"], 3);
    assert!(components["test_path_factor"].as_f64().unwrap() > 0.9);
}

#[test]
fn summary_emits_one_jsonl_row_per_candidate() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    // override the json formatter set by doctor_args
    let pos = args.iter().position(|a| a == "json").unwrap();
    args[pos] = "summary".into();
    let assert = run(args).code(3);
    let out = stdout(&assert);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 1);
    let row: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(row["sha"], fx.unswept_sha.as_str());
    assert_eq!(row["action"], "sweep");
}

#[test]
fn text_summary_emits_tab_separated_rows() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    let pos = args.iter().position(|a| a == "json").unwrap();
    args[pos] = "text-summary".into();
    let assert = run(args).code(3);
    let out = stdout(&assert);
    let fields: Vec<&str> = out.lines().next().unwrap().split('\t').collect();
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0], &fx.unswept_sha[..12]);
    assert_eq!(fields[1], "sweep");
    assert_eq!(fields[3], "Deflake app_SUITE: wait_for the condition");
}

#[test]
fn text_table_renders_with_the_suppression_footer() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    let pos = args.iter().position(|a| a == "json").unwrap();
    args[pos] = "text".into();
    let assert = run(args).code(3);
    let out = stdout(&assert);
    assert!(out.contains("SUBJECT"), "table header missing: {out}");
    assert!(out.contains("Deflake app_SUITE"), "row missing: {out}");
    assert!(
        out.contains("suppressed 1 already-cascaded commit; walked 3 across 1 source branch"),
        "footer missing: {out}"
    );
}

#[test]
fn with_branches_adds_a_containment_column() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    let pos = args.iter().position(|a| a == "json").unwrap();
    args[pos] = "text".into();
    args.push("--with-branches".into());
    let assert = run(args).code(3);
    let out = stdout(&assert);
    assert!(out.contains("BRANCHES"), "column missing: {out}");
    assert!(out.contains("main"), "containment missing: {out}");
}

// --- cache behaviour ---

fn run_with_env(args: &[String], env: &[(&str, &str)]) -> assert_cmd::assert::Assert {
    let mut cmd = assert_cmd::Command::cargo_bin("backhopper").unwrap();
    cmd.env("BACKHOPPER_FORMATTER", "text");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.args(args).assert()
}

#[test]
fn force_cache_serves_the_second_run_from_the_entry() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    args.push("-vv".into());
    let first = run_with_env(&args, &[("BACKHOPPER_FORCE_CACHE", "1")]);
    first.code(3);
    assert!(
        fx.dir
            .path()
            .join("snapshots/.siblings_doctor_cache")
            .is_dir(),
        "cache directory was not created"
    );
    let second = run_with_env(&args, &[("BACKHOPPER_FORCE_CACHE", "1")]);
    let err = stderr(&second.code(3));
    assert!(err.contains("cache hit"), "no cache hit logged: {err}");
}

#[test]
fn a_moved_source_tip_invalidates_the_cached_entry() {
    let fx = DoctorFixture::new("rabbitmq");
    let args = fx.doctor_args();
    run_with_env(&args, &[("BACKHOPPER_FORCE_CACHE", "1")]).code(3);
    // a new unswept fix lands on main; a stale cache would hide it
    let repo = fx.repo();
    write(
        &repo,
        "test/app_SUITE.erl",
        "-module(app_SUITE).\n-export([t/1]).\nt(_) -> wait_for(ok), done.\n",
    );
    git(&repo, &["add", "-A"]);
    commit(
        &repo,
        "Fix another flake in app_SUITE",
        "2025-08-09T10:00:00Z",
    );
    let new_sha = head(&repo);

    let assert = run_with_env(&args, &[("BACKHOPPER_FORCE_CACHE", "1")]).code(3);
    let body = parse_envelope(&stdout(&assert));
    let candidates = body["data"]["candidates"].as_array().unwrap();
    assert!(
        candidates.iter().any(|c| c["sha"] == new_sha.as_str()),
        "the fresh fix is missing: stale cache served"
    );
}

#[test]
fn no_cache_env_wins_over_force_cache() {
    let fx = DoctorFixture::new("rabbitmq");
    let args = fx.doctor_args();
    run_with_env(
        &args,
        &[
            ("BACKHOPPER_FORCE_CACHE", "1"),
            ("BACKHOPPER_NO_CACHE", "1"),
        ],
    )
    .code(3);
    assert!(
        !fx.dir
            .path()
            .join("snapshots/.siblings_doctor_cache")
            .exists(),
        "cache directory exists despite BACKHOPPER_NO_CACHE"
    );
}

#[test]
fn no_cache_flag_skips_the_cache_entirely() {
    let fx = DoctorFixture::new("rabbitmq");
    let mut args = fx.doctor_args();
    args.push("--no-cache".into());
    run_with_env(&args, &[("BACKHOPPER_FORCE_CACHE", "1")]).code(3);
    assert!(
        !fx.dir
            .path()
            .join("snapshots/.siblings_doctor_cache")
            .exists(),
        "cache directory exists despite --no-cache"
    );
}

#[test]
fn vocabulary_file_replaces_the_family_default() {
    let fx = DoctorFixture::new("rabbitmq");
    let vocab_path = fx.dir.path().join("vocab.txt");
    // matches only the feature commit the default vocabulary skips
    std::fs::write(&vocab_path, "# custom terms\ng/0\n").unwrap();
    let mut args = fx.doctor_args();
    args.push("--vocabulary-file-path".into());
    args.push(vocab_path.to_string_lossy().into_owned());
    let assert = run(args).code(3);
    let body = parse_envelope(&stdout(&assert));
    assert_eq!(body["data"]["vocabulary_source"], "file");
    let candidates = body["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["subject"], "Add g/0");
}
