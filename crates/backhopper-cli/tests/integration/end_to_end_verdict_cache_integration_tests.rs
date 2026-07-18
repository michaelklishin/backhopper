// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end coverage of the verdict cache through the process
//! boundary. Every test forces the cache on (test binaries are debug
//! builds, which bypass by default) and reads the per-run summary
//! line for exact hit/miss accounting.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::assert::Assert;
use tempfile::TempDir;

use backhopper_test_support::toml_path;

use crate::helpers::cli::{stderr, stdout};

/// A workspace holding a demo project repo (with an include-bearing
/// module), tags, a series, and generated snapshots.
struct CacheFixture {
    dir: TempDir,
    fix_sha: String,
}

const DEMO_HRL: &str = "-define(GREETING, \"hello\").\n";

const DEMO_ERL_BASE: &str = r#"-module(demo_mod).
-include("demo.hrl").
-export([greet/1]).

%% A realistic preamble keeps the fix's hunk away from the start of
%% the file, where differs may anchor context differently.
preamble_one() ->
    one.

preamble_two() ->
    two.

greet(Name) ->
    [?GREETING, Name].

trailing_one() ->
    one.

trailing_two() ->
    two.
"#;

// the fix touches the macro-using line, far from the padding a branch adds up top, so its hunk reaches ?GREETING
const DEMO_ERL_FIXED: &str = r#"-module(demo_mod).
-include("demo.hrl").
-export([greet/1]).

%% A realistic preamble keeps the fix's hunk away from the start of
%% the file, where differs may anchor context differently.
preamble_one() ->
    one.

preamble_two() ->
    two.

greet(Name) ->
    [?GREETING, Name, fixed].

trailing_one() ->
    one.

trailing_two() ->
    two.
"#;

impl CacheFixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        write(&repo, "include/demo.hrl", DEMO_HRL);
        write(&repo, "src/demo_mod.erl", DEMO_ERL_BASE);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        git(&repo, &["tag", "v1.0.0"]);

        // the fix on main whose cherry-picks the cascade tests replay
        write(&repo, "src/demo_mod.erl", DEMO_ERL_FIXED);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "fix the tail"]);
        let fix_sha = head(&repo);

        let config = format!(
            r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[cache]
ttl_days = 42

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "stable"
pins = [
  {{ project = "demo", tag = "v1.0.0" }},
]
"#,
            toml_path(&repo)
        );
        std::fs::write(dir.path().join("backhopper.toml"), config).unwrap();
        let fixture = Self { dir, fix_sha };
        fixture
            .run(["snapshots", "generate", "--project", "demo"])
            .success();
        fixture
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

    fn verdict_cache_dir(&self) -> PathBuf {
        self.dir.path().join("snapshots/.verdict_cache")
    }

    /// Run with the cache forced on and info logging enabled.
    fn run<I, S>(&self, args: I) -> Assert
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_with_env(args, &[("BACKHOPPER_FORCE_CACHE", "1")])
    }

    fn run_with_env<I, S>(&self, args: I, env: &[(&str, &str)]) -> Assert
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = assert_cmd::Command::cargo_bin("backhopper").unwrap();
        cmd.env("BACKHOPPER_FORMATTER", "json");
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.arg("--config-file-path")
            .arg(self.config())
            .arg("-v")
            .args(args);
        cmd.assert()
    }

    fn check_commit_args(&self, sha: &str) -> Vec<String> {
        vec![
            "check".into(),
            "commit".into(),
            "--project".into(),
            "demo".into(),
            "--tag".into(),
            "v1.0.0".into(),
            "--repo-dir-path".into(),
            self.repo().to_string_lossy().into_owned(),
            sha.to_owned(),
        ]
    }
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
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

/// The per-run summary line, stripped of the tracing prefix.
fn cache_line(assert: &Assert) -> String {
    stderr(assert)
        .lines()
        .find(|l| l.contains("verdict cache:"))
        .map(|l| {
            let idx = l.find("verdict cache:").unwrap();
            l[idx..].to_owned()
        })
        .unwrap_or_default()
}

#[test]
fn the_second_run_hits_l1_with_a_byte_identical_envelope() {
    let fx = CacheFixture::new();
    let first = fx.run(fx.check_commit_args(&fx.fix_sha));
    assert_eq!(
        cache_line(&first),
        "verdict cache: 0 L1 hits, 0 L2 hits (0 aliases minted), 1 misses, 0 bypassed"
    );
    let second = fx.run(fx.check_commit_args(&fx.fix_sha));
    assert_eq!(
        cache_line(&second),
        "verdict cache: 1 L1 hits, 0 L2 hits (0 aliases minted), 0 misses, 0 bypassed"
    );
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn a_fresh_clone_of_the_same_content_hits() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    // re-check from another checkout: same SHAs, different working directory
    let clone_dir = fx.dir.path().join("clone");
    git(
        fx.dir.path(),
        &["clone", "-q", "repo", clone_dir.to_str().unwrap()],
    );
    let mut args = fx.check_commit_args(&fx.fix_sha);
    let repo_idx = args.iter().position(|a| a == "--repo-dir-path").unwrap();
    args[repo_idx + 1] = clone_dir.to_string_lossy().into_owned();
    let second = fx.run(args);
    assert_eq!(
        cache_line(&second),
        "verdict cache: 1 L1 hits, 0 L2 hits (0 aliases minted), 0 misses, 0 bypassed"
    );
}

#[test]
fn a_config_edit_busts_the_cache() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let config_path = fx.dir.path().join("backhopper.toml");
    let mut body = std::fs::read_to_string(&config_path).unwrap();
    body.push_str("\n# a comment edit over-invalidates; that is the correct trade\n");
    std::fs::write(&config_path, body).unwrap();
    let second = fx.run(fx.check_commit_args(&fx.fix_sha));
    assert!(
        cache_line(&second).contains("1 misses"),
        "expected a miss after a config edit: {}",
        cache_line(&second)
    );
}

#[test]
fn an_offset_shifted_cherry_pick_hits_l2_and_mints_an_alias() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let repo = fx.repo();
    // a stable branch whose padding shifts the fix's hunks down
    git(&repo, &["checkout", "-q", "-b", "v1.x", "v1.0.0"]);
    let padded = format!("%% padding one\n%% padding two\n%% padding three\n{DEMO_ERL_BASE}");
    write(&repo, "src/demo_mod.erl", &padded);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "padding up top"]);
    git(&repo, &["cherry-pick", "-x", &fx.fix_sha]);
    let pick_sha = head(&repo);
    git(&repo, &["checkout", "-q", "main"]);

    let first_sight = fx.run(fx.check_commit_args(&pick_sha));
    assert_eq!(
        cache_line(&first_sight),
        "verdict cache: 0 L1 hits, 1 L2 hits (1 aliases minted), 0 misses, 0 bypassed"
    );
    let second_sight = fx.run(fx.check_commit_args(&pick_sha));
    assert_eq!(
        cache_line(&second_sight),
        "verdict cache: 1 L1 hits, 0 L2 hits (0 aliases minted), 0 misses, 0 bypassed"
    );
}

#[test]
fn an_adapted_pick_misses_l2() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let repo = fx.repo();
    git(&repo, &["checkout", "-q", "-b", "v1-adapted", "v1.0.0"]);
    // same shape as the fix, drifted content: must re-evaluate
    let adapted = DEMO_ERL_FIXED.replace("Name, fixed", "Name, adapted");
    write(&repo, "src/demo_mod.erl", &adapted);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "fix the tail (adapted)"]);
    let adapted_sha = head(&repo);
    git(&repo, &["checkout", "-q", "main"]);

    let sight = fx.run(fx.check_commit_args(&adapted_sha));
    assert!(
        cache_line(&sight).contains("0 L2 hits") && cache_line(&sight).contains("1 misses"),
        "an adapted pick must miss: {}",
        cache_line(&sight)
    );
}

#[test]
fn macro_environment_drift_misses_l2() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let repo = fx.repo();
    // identical patch bytes, but the included header differs on this branch, so the macro tables differ
    git(&repo, &["checkout", "-q", "-b", "v1-macro", "v1.0.0"]);
    write(
        &repo,
        "include/demo.hrl",
        "-define(GREETING, \"servus\").\n",
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "different greeting"]);
    git(&repo, &["cherry-pick", "-x", &fx.fix_sha]);
    let pick_sha = head(&repo);
    git(&repo, &["checkout", "-q", "main"]);

    let sight = fx.run(fx.check_commit_args(&pick_sha));
    assert!(
        cache_line(&sight).contains("0 L2 hits") && cache_line(&sight).contains("1 misses"),
        "macro-environment drift must miss: {}",
        cache_line(&sight)
    );
}

#[test]
fn header_drift_the_patch_never_references_still_hits() {
    let fx = CacheFixture::new();
    let repo = fx.repo();
    // a tail fix whose hunk reaches no macro at all
    write(
        &repo,
        "src/demo_mod.erl",
        &DEMO_ERL_FIXED.replace(
            "trailing_two() ->\n    two.",
            "trailing_two() ->\n    {two, tail_fixed}.",
        ),
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "fix the tail"]);
    let tail_sha = head(&repo);
    let _ = fx.run(fx.check_commit_args(&tail_sha));

    // the included header drifts between branches, but the patch never references its macros
    git(&repo, &["checkout", "-q", "-b", "v1-tail", "v1.0.0"]);
    write(
        &repo,
        "include/demo.hrl",
        "-define(GREETING, \"servus\").\n",
    );
    write(&repo, "src/demo_mod.erl", DEMO_ERL_FIXED);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "diverge header and greet"]);
    git(&repo, &["cherry-pick", "-x", &tail_sha]);
    let pick_sha = head(&repo);
    git(&repo, &["checkout", "-q", "main"]);

    let sight = fx.run(fx.check_commit_args(&pick_sha));
    assert_eq!(
        cache_line(&sight),
        "verdict cache: 0 L1 hits, 1 L2 hits (1 aliases minted), 0 misses, 0 bypassed",
        "unreferenced header drift must not bust the cascade reuse"
    );
}

#[test]
fn explain_renders_identically_from_hit_and_miss() {
    let fx = CacheFixture::new();
    let mut args = fx.check_commit_args(&fx.fix_sha);
    args.insert(2, "--explain".into());
    let miss = fx.run(args.clone());
    assert!(cache_line(&miss).contains("1 misses"));
    let hit = fx.run(args);
    assert!(cache_line(&hit).contains("1 L1 hits"));
    assert_eq!(stdout(&miss), stdout(&hit));
}

#[test]
fn no_cache_flag_skips_reads_and_writes() {
    let fx = CacheFixture::new();
    let mut args = fx.check_commit_args(&fx.fix_sha);
    args.insert(2, "--no-cache".into());
    let run = fx.run(args);
    assert!(
        cache_line(&run).contains("1 bypassed"),
        "expected a bypass: {}",
        cache_line(&run)
    );
    assert!(
        !fx.verdict_cache_dir().exists(),
        "a bypassed run must not write"
    );
}

#[test]
fn the_no_cache_env_var_wins_over_force_cache() {
    let fx = CacheFixture::new();
    let _ = fx.run_with_env(
        fx.check_commit_args(&fx.fix_sha),
        &[
            ("BACKHOPPER_FORCE_CACHE", "1"),
            ("BACKHOPPER_NO_CACHE", "1"),
        ],
    );
    assert!(!fx.verdict_cache_dir().exists());
}

#[test]
fn bypass_flags_neither_read_nor_write() {
    let fx = CacheFixture::new();
    // seed an entry, then run with a bypass flag: the entry must not be served and nothing new written
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let count_entries = || walkdir_count(&fx.verdict_cache_dir());
    let seeded = count_entries();
    let mut args = fx.check_commit_args(&fx.fix_sha);
    args.insert(2, "--suggest-prereqs".into());
    let run = fx.run(args);
    assert!(
        !stderr(&run).contains("verdict cache:"),
        "bypass shapes do not report"
    );
    assert_eq!(count_entries(), seeded);
}

fn walkdir_count(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn batch_pairs_hit_on_the_second_run() {
    let fx = CacheFixture::new();
    let commits_file = fx.dir.path().join("candidates.txt");
    std::fs::write(&commits_file, format!("{}\n", fx.fix_sha)).unwrap();
    let batch_args = || {
        vec![
            "check".to_owned(),
            "batch".to_owned(),
            "--series".to_owned(),
            "stable".to_owned(),
            "--repo-dir-path".to_owned(),
            fx.repo().to_string_lossy().into_owned(),
            "--commits-file-path".to_owned(),
            commits_file.to_string_lossy().into_owned(),
        ]
    };
    let first = fx.run(batch_args());
    assert!(cache_line(&first).contains("1 misses"));
    let second = fx.run(batch_args());
    assert!(
        cache_line(&second).contains("1 L1 hits"),
        "batch must hit on the second run: {}",
        cache_line(&second)
    );
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn a_check_commit_warm_up_serves_the_batch() {
    let fx = CacheFixture::new();
    // a single-commit triage seeds the entry a later batch over the same series consumes
    let mut args = fx.check_commit_args(&fx.fix_sha);
    let project_idx = args.iter().position(|a| a == "--project").unwrap();
    // series-targeted so the key matches the batch's series key
    args.splice(
        project_idx..project_idx + 4,
        ["--series".to_owned(), "stable".to_owned()],
    );
    let _ = fx.run(args);
    let commits_file = fx.dir.path().join("candidates.txt");
    std::fs::write(&commits_file, format!("{}\n", fx.fix_sha)).unwrap();
    let batch = fx.run([
        "check",
        "batch",
        "--series",
        "stable",
        "--repo-dir-path",
        fx.repo().to_str().unwrap(),
        "--commits-file-path",
        commits_file.to_str().unwrap(),
    ]);
    assert!(
        cache_line(&batch).contains("1 L1 hits"),
        "the batch must reuse the check commit warm-up: {}",
        cache_line(&batch)
    );
}

#[test]
fn bisect_caches_one_entry_per_tag_and_reuses_them() {
    let fx = CacheFixture::new();
    let repo = fx.repo();
    // a second tag so the bisect walk has two rows
    git(&repo, &["tag", "v1.1.0"]);
    fx.run(["snapshots", "generate", "--project", "demo"])
        .success();
    let bisect_args = || {
        vec![
            "bisect".to_owned(),
            "commit".to_owned(),
            "--project".to_owned(),
            "demo".to_owned(),
            "--repo-dir-path".to_owned(),
            repo.to_string_lossy().into_owned(),
            fx.fix_sha.clone(),
        ]
    };
    let first = fx.run(bisect_args());
    assert!(
        cache_line(&first).contains("2 misses"),
        "two tags, two misses: {}",
        cache_line(&first)
    );
    let second = fx.run(bisect_args());
    assert!(
        cache_line(&second).contains("2 L1 hits"),
        "two tags, two hits: {}",
        cache_line(&second)
    );
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn evaluation_is_deterministic_without_the_cache() {
    let fx = CacheFixture::new();
    let mut args = fx.check_commit_args(&fx.fix_sha);
    args.insert(2, "--no-cache".into());
    let first = fx.run(args.clone());
    let second = fx.run(args);
    assert_eq!(stdout(&first), stdout(&second));
}

#[test]
fn empty_diffs_are_cached_uniformly() {
    let fx = CacheFixture::new();
    let repo = fx.repo();
    write(&repo, "README.md", "docs only\n");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "docs"]);
    let docs_sha = head(&repo);
    let first = fx.run(fx.check_commit_args(&docs_sha));
    assert!(cache_line(&first).contains("1 misses"));
    let second = fx.run(fx.check_commit_args(&docs_sha));
    assert!(cache_line(&second).contains("1 L1 hits"));
}

#[test]
fn cache_verbs_round_trip_the_seeded_entries() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));

    let stats = fx.run(["cache", "stats"]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&stats)).unwrap();
    assert_eq!(body["command"], "cache stats");
    assert_eq!(body["data"]["by_input"]["entries"], 1);
    assert_eq!(body["data"]["by_content"]["entries"], 1);

    let list = fx.run(["cache", "list", "--commit", &fx.fix_sha[..12]]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&list)).unwrap();
    let rows = body["data"]["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let key = rows[0]["key"].as_str().unwrap().to_owned();

    let show = fx.run(["cache", "show", &key[..10], "--full"]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(body["data"]["key"], key.as_str());
    assert_eq!(body["data"]["key_inputs"]["commit"], fx.fix_sha.as_str());
    assert!(body["data"]["value"]["verdict"].is_object());

    let evict = fx.run(["cache", "evict", "--commit", &fx.fix_sha[..12]]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&evict)).unwrap();
    assert_eq!(body["data"]["removed_entries"], 1);

    let clear = fx.run(["cache", "clear"]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&clear)).unwrap();
    assert_eq!(body["data"]["removed_entries"], 1);

    let stats = fx.run(["cache", "stats"]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&stats)).unwrap();
    assert_eq!(body["data"]["total_bytes"], 0);
}

#[test]
fn evict_by_commit_takes_alias_entries_with_it() {
    let fx = CacheFixture::new();
    let _ = fx.run(fx.check_commit_args(&fx.fix_sha));
    let repo = fx.repo();
    git(&repo, &["checkout", "-q", "-b", "v1-evict", "v1.0.0"]);
    git(&repo, &["cherry-pick", "-x", &fx.fix_sha]);
    let pick_sha = head(&repo);
    git(&repo, &["checkout", "-q", "main"]);
    let sight = fx.run(fx.check_commit_args(&pick_sha));
    assert!(cache_line(&sight).contains("1 aliases minted"));

    // the alias entry's pre-image names the pick SHA
    let evict = fx.run(["cache", "evict", "--commit", &pick_sha[..12]]);
    let body: serde_json::Value = serde_json::from_str(&stdout(&evict)).unwrap();
    assert_eq!(body["data"]["removed_entries"], 1);
    // the pick now misses L1 (the alias is gone) but re-aliases from the surviving content entry
    let resight = fx.run(fx.check_commit_args(&pick_sha));
    assert!(cache_line(&resight).contains("1 L2 hits"));
}
