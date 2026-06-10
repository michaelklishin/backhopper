// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The design's cost budget: a ~500-commit first-parent window must
//! complete in under five seconds. The fixture is built with one
//! `git fast-import` stream so the setup does not dwarf the
//! measurement.

use std::fmt::Write;
use std::io::Write as IoWrite;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

use crate::helpers::cli::{run, stdout};

const WINDOW_COMMITS: usize = 500;
const BUDGET: Duration = Duration::from_secs(5);

fn fast_import_stream() -> String {
    let mut s = String::new();
    let base_time = 1_750_000_000i64;

    // blobs
    s.push_str("blob\nmark :1\ndata 14\nbase content\n\n");
    s.push_str("blob\nmark :2\ndata 11\nsuite base\n\n");

    // base commit on main
    write_commit(
        &mut s,
        "refs/heads/main",
        100,
        base_time,
        "init",
        None,
        &[("src/app.erl", 1), ("test/app_SUITE.erl", 2)],
    );

    // v1.x forks from base and gets the release tag
    write_commit(
        &mut s,
        "refs/heads/v1.x",
        101,
        base_time + 100,
        "release prep",
        Some(100),
        &[],
    );
    s.push_str("reset refs/tags/v1.0.0\nfrom :101\n\n");

    // the 500-commit window on main, every 16th a vocabulary match
    let mut mark = 200;
    for i in 0..WINDOW_COMMITS {
        let blob_mark = mark;
        let content = format!("content for commit {i}\n");
        let _ = write!(
            s,
            "blob\nmark :{blob_mark}\ndata {}\n{content}\n",
            content.len()
        );
        mark += 1;
        let commit_mark = mark;
        mark += 1;
        let (subject, path) = if i % 16 == 0 {
            (
                format!("Fix a flake in app_SUITE (round {i})"),
                "test/app_SUITE.erl",
            )
        } else {
            (format!("Refactor part {i}"), "src/app.erl")
        };
        write_commit(
            &mut s,
            "refs/heads/main",
            commit_mark,
            base_time + 1_000 + (i as i64) * 60,
            &subject,
            None,
            &[(path, blob_mark)],
        );
    }
    s.push_str("done\n");
    s
}

fn write_commit(
    s: &mut String,
    branch: &str,
    mark: usize,
    time: i64,
    message: &str,
    from: Option<usize>,
    files: &[(&str, usize)],
) {
    let _ = write!(s, "commit {branch}\nmark :{mark}\n");
    let _ = write!(
        s,
        "author t <t@e> {time} +0000\ncommitter t <t@e> {time} +0000\n"
    );
    let _ = write!(s, "data {}\n{message}\n", message.len() + 1);
    if let Some(parent) = from {
        let _ = writeln!(s, "from :{parent}");
    }
    for (path, blob_mark) in files {
        let _ = writeln!(s, "M 100644 :{blob_mark} {path}");
    }
    s.push('\n');
}

fn build_fixture(dir: &Path) {
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(ok.success());
    let mut child = Command::new("git")
        .args(["fast-import", "--quiet", "--done"])
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(fast_import_stream().as_bytes())
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "fast-import failed");
    // fast-import leaves the worktree empty; the verb only reads the odb
    let config = r#"
config_version = 1

[defaults]
snapshot_dir = "snapshots"

[[project]]
name        = "demo"
kind        = "self"
family      = "rabbitmq"
tag_pattern = "v*"

[[series]]
name = "demo-1.0"
pins = [
  { project = "demo", branch = "v1.x" },
]
"#;
    std::fs::write(dir.join("backhopper.toml"), config).unwrap();
    std::fs::create_dir_all(dir.join("snapshots")).unwrap();
}

#[test]
fn five_hundred_commit_window_stays_inside_the_budget() {
    let dir = TempDir::new().unwrap();
    build_fixture(dir.path());
    let args = [
        "--config-file-path".to_owned(),
        dir.path()
            .join("backhopper.toml")
            .to_string_lossy()
            .into_owned(),
        "--formatter".to_owned(),
        "json".to_owned(),
        "siblings".to_owned(),
        "doctor".to_owned(),
        "--series".to_owned(),
        "demo-1.0".to_owned(),
        "--repo-dir-path".to_owned(),
        dir.path().join("repo").to_string_lossy().into_owned(),
        "--top".to_owned(),
        "50".to_owned(),
    ];
    let started = Instant::now();
    let assert = run(args).code(3);
    let elapsed = started.elapsed();
    let body: serde_json::Value = serde_json::from_str(&stdout(&assert)).unwrap();
    assert_eq!(body["data"]["walked_count"], WINDOW_COMMITS as i64);
    // every 16th of 500 commits matches the vocabulary
    let expected_candidates = WINDOW_COMMITS.div_ceil(16);
    assert_eq!(
        body["data"]["candidates"].as_array().unwrap().len(),
        expected_candidates
    );
    assert!(
        elapsed < BUDGET,
        "siblings doctor took {elapsed:?}, over the {BUDGET:?} budget"
    );
}
