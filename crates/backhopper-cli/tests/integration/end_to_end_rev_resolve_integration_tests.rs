// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::process::Command as Std;

use serde_json::Value;

use crate::helpers::cli::{run, run_succeeds, stderr, stdout};
use backhopper_test_support::GitRepoFixture;

fn fixture() -> (GitRepoFixture, String) {
    let repo = GitRepoFixture::new();
    repo.write_file("a.txt", "hi\n");
    repo.commit("a");
    let head = Std::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo.dir.path())
        .output()
        .unwrap();
    let head_sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    (repo, head_sha)
}

#[test]
fn rev_resolve_expands_short_prefix_to_full_sha_in_json_payload() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "json",
        &head[..10],
    ]));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["command"], "rev resolve");
    assert_eq!(v["data"]["input"], head[..10]);
    assert_eq!(v["data"]["resolved"], head);
    assert_eq!(v["data"]["object_kind"], "commit");
}

#[test]
fn rev_resolve_accepts_full_forty_char_input() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "json",
        &head,
    ]));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["data"]["resolved"], head);
}

#[test]
fn rev_resolve_summary_formatter_emits_single_line_jsonl_payload() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "summary",
        &head[..8],
    ]));
    let trimmed = out.trim();
    assert!(
        !trimmed.contains('\n'),
        "expected single line, got: {trimmed:?}"
    );
    let v: Value = serde_json::from_str(trimmed).expect("jsonl row is valid json");
    assert_eq!(v["resolved"], head);
}

#[test]
fn rev_resolve_text_formatter_emits_aligned_key_value_block() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "text",
        &head[..10],
    ]));
    assert!(out.contains("input:"), "expected input key, got: {out}");
    assert!(
        out.contains("resolved:"),
        "expected resolved key, got: {out}"
    );
    assert!(
        out.contains("object_kind:"),
        "expected object_kind key, got: {out}"
    );
    assert!(
        out.contains(&head[..10]),
        "expected input echoed, got: {out}"
    );
    assert!(out.contains(&head), "expected resolved sha, got: {out}");
}

#[test]
fn rev_resolve_text_summary_prints_bare_full_sha() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "text-summary",
        &head[..7],
    ]));
    assert_eq!(out.trim(), head);
}

#[test]
fn rev_resolve_subject_flag_emits_commit_subject_in_payload() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--subject",
        "--formatter",
        "json",
        &head[..8],
    ]));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["data"]["subject"], "a");
}

#[test]
fn rev_resolve_fails_non_zero_for_missing_prefix() {
    let (repo, _) = fixture();
    let assert = run([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "json",
        "deadbeef",
    ])
    .failure();
    let err = stderr(&assert);
    assert!(
        err.contains("deadbeef"),
        "expected prefix echoed in stderr, got: {err}"
    );
    assert!(
        err.contains("not found"),
        "expected 'not found' message, got: {err}"
    );
    let out = stdout(&assert);
    let v: Value = serde_json::from_str(&out).expect("json envelope still emitted");
    assert_eq!(v["data"]["input"], "deadbeef");
}

#[test]
fn rev_resolve_rejects_six_char_prefix_at_clap_parse() {
    let (repo, _) = fixture();
    let err = stderr(
        &run([
            "rev",
            "resolve",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "abc123",
        ])
        .failure(),
    );
    assert!(
        err.contains("too short") || err.contains("7 to 40"),
        "expected short-prefix rejection, got: {err}"
    );
}

#[test]
fn rev_resolve_rejects_non_hex_input() {
    let (repo, _) = fixture();
    let err = stderr(
        &run([
            "rev",
            "resolve",
            "--repo-dir-path",
            repo.dir.path().to_str().unwrap(),
            "abcdefgh",
        ])
        .failure(),
    );
    assert!(
        err.contains("non-hex") || err.contains("hex"),
        "expected non-hex rejection, got: {err}"
    );
}

#[test]
fn rev_resolve_normalises_uppercase_to_lowercase_in_input_echo() {
    let (repo, head) = fixture();
    let out = stdout(&run_succeeds([
        "rev",
        "resolve",
        "--repo-dir-path",
        repo.dir.path().to_str().unwrap(),
        "--formatter",
        "json",
        &head[..10].to_ascii_uppercase(),
    ]));
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["data"]["input"], head[..10]);
    assert_eq!(v["data"]["resolved"], head);
}

#[test]
fn rev_resolve_fails_cleanly_on_non_git_directory() {
    let work = tempfile::TempDir::new().unwrap();
    let err = stderr(
        &run([
            "rev",
            "resolve",
            "--repo-dir-path",
            work.path().to_str().unwrap(),
            "abcdef12",
        ])
        .failure(),
    );
    assert!(
        err.contains("not a git repository"),
        "expected not-a-git-repository message, got: {err}"
    );
}
