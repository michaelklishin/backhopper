// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The cache's perf gate: a warm `check batch` must beat its own
//! cold run and stay inside a generous debug-build bound. The design
//! target (300 ms warm for a 60-pair batch) is a release-build
//! number; this gate catches order-of-magnitude regressions, not
//! milliseconds.

use std::fmt::Write;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::helpers::cli::stderr;

const COMMITS: usize = 20;
const WARM_BUDGET: Duration = Duration::from_secs(5);

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@e")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@e")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn a_warm_batch_beats_its_cold_run_and_stays_inside_the_budget() {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(
        repo.join("src/demo_mod.erl"),
        "-module(demo_mod).\n-export([f/0]).\nf() -> ok.\n",
    )
    .unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["tag", "v1.0.0"]);
    let mut commits = String::new();
    for i in 0..COMMITS {
        std::fs::write(
            repo.join("src/demo_mod.erl"),
            format!("-module(demo_mod).\n-export([f/0]).\nf() -> {i}.\n"),
        )
        .unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", &format!("change {i}")]);
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        let _ = writeln!(commits, "{}", String::from_utf8(out.stdout).unwrap().trim());
    }
    let config = format!(
        r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name    = "demo"
git_url = "{}"

[[series]]
name = "stable"
pins = [
  {{ project = "demo", tag = "v1.0.0" }},
]
"#,
        repo.display()
    );
    let cfg = dir.path().join("backhopper.toml");
    std::fs::write(&cfg, config).unwrap();
    let commits_file = dir.path().join("candidates.txt");
    std::fs::write(&commits_file, commits).unwrap();

    let run_batch = || {
        let mut cmd = assert_cmd::Command::cargo_bin("backhopper").unwrap();
        cmd.env("BACKHOPPER_FORCE_CACHE", "1")
            .env("BACKHOPPER_FORMATTER", "json")
            .args([
                "--config-file-path",
                cfg.to_str().unwrap(),
                "-v",
                "check",
                "batch",
                "--series",
                "stable",
                "--repo-dir-path",
                repo.to_str().unwrap(),
                "--commits-file-path",
                commits_file.to_str().unwrap(),
            ]);
        let started = Instant::now();
        let assert = cmd.assert();
        (started.elapsed(), assert)
    };

    // generate snapshots outside the measurement
    assert_cmd::Command::cargo_bin("backhopper")
        .unwrap()
        .args([
            "--config-file-path",
            cfg.to_str().unwrap(),
            "snapshots",
            "generate",
            "--project",
            "demo",
        ])
        .assert()
        .success();

    let (cold, cold_assert) = run_batch();
    assert!(stderr(&cold_assert).contains(&format!("{COMMITS} misses")));
    let (warm, warm_assert) = run_batch();
    assert!(stderr(&warm_assert).contains(&format!("{COMMITS} L1 hits")));
    assert!(
        warm <= cold,
        "the warm run ({warm:?}) must not be slower than the cold run ({cold:?})"
    );
    assert!(
        warm < WARM_BUDGET,
        "warm batch took {warm:?}, over the {WARM_BUDGET:?} budget"
    );
}
