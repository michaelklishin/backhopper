// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Walk-correctness fixture for `pr_commits_for`. Builds a tiny repo
//! with a known 2-parent merge structure and asserts the walk
//! enumerates exactly the PR-branch commits.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::str;

use tempfile::TempDir;

use backhopper_core::git::{GitRepo, pr_commits_for};
use backhopper_core::model::names::CommitSha;
use backhopper_core::model::pr_commit::PrCommitKind;

struct Fixture {
    _dir: TempDir,
    repo: GitRepo,
}

fn git<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(cwd: &Path, args: I) -> Output {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        str::from_utf8(&out.stderr).unwrap_or("(stderr not utf-8)"),
        out.status,
    );
    out
}

fn write(dir: &Path, rel: &str, body: &str) {
    let full = dir.join(rel);
    if let Some(p) = full.parent() {
        fs::create_dir_all(p).unwrap();
    }
    fs::write(&full, body).unwrap();
}

fn head_sha(dir: &Path) -> CommitSha {
    let out = git(dir, ["rev-parse", "HEAD"]);
    let s = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    CommitSha::new(s).unwrap()
}

fn commit(dir: &Path, subject: &str) {
    git(dir, ["add", "-A"]);
    git(dir, ["commit", "--no-gpg-sign", "-m", subject]);
}

fn fixture_with_merge() -> (Fixture, CommitSha, Vec<CommitSha>) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().to_path_buf();
    git(&path, ["init", "--initial-branch", "main"]);
    write(&path, "README.md", "init\n");
    commit(&path, "initial");
    git(&path, ["checkout", "-b", "feature"]);
    write(&path, "src/a.txt", "a\n");
    commit(&path, "Add src/a.txt");
    let pr1 = head_sha(&path);
    write(&path, "src/b.txt", "b\n");
    commit(&path, "Add src/b.txt");
    let pr2 = head_sha(&path);
    write(&path, "src/a.txt", "a-fixed\n");
    commit(&path, "Resolve conflicts");
    let pr3 = head_sha(&path);
    git(&path, ["checkout", "main"]);
    git(
        &path,
        ["merge", "--no-ff", "--no-edit", "--no-gpg-sign", "feature"],
    );
    let merge = head_sha(&path);
    let repo = GitRepo::open(&path).expect("open");
    (Fixture { _dir: dir, repo }, merge, vec![pr1, pr2, pr3])
}

#[test]
fn walks_exactly_pr_branch_commits_in_topological_order() {
    let (fx, merge, expected_pr) = fixture_with_merge();
    let walk = pr_commits_for(&fx.repo, &merge)
        .expect("walks ok")
        .expect("merge is 2-parent");
    let got: Vec<CommitSha> = walk.iter().map(|c| c.sha.clone()).collect();
    assert_eq!(got, expected_pr);
}

#[test]
fn last_commit_is_classified_as_conflict_resolution() {
    let (fx, merge, _) = fixture_with_merge();
    let walk = pr_commits_for(&fx.repo, &merge).unwrap().unwrap();
    let last = walk.last().expect("at least one PR-branch commit");
    assert_eq!(last.subject, "Resolve conflicts");
    assert_eq!(last.kind, PrCommitKind::ConflictResolution);
}

#[test]
fn early_pr_commits_are_substantive_because_no_prior_union() {
    let (fx, merge, _) = fixture_with_merge();
    let walk = pr_commits_for(&fx.repo, &merge).unwrap().unwrap();
    assert_eq!(walk[0].kind, PrCommitKind::Substantive);
    assert_eq!(walk[1].kind, PrCommitKind::Substantive);
}

#[test]
fn non_merge_commit_yields_none() {
    let (fx, _, pr_shas) = fixture_with_merge();
    let non_merge = pr_shas.last().unwrap().clone();
    let result = pr_commits_for(&fx.repo, &non_merge).unwrap();
    assert!(
        result.is_none(),
        "non-merge SHA must return None, got {result:?}"
    );
}

#[test]
fn pr_commit_carries_short_subject_string() {
    let (fx, merge, _) = fixture_with_merge();
    let walk = pr_commits_for(&fx.repo, &merge).unwrap().unwrap();
    for c in &walk {
        assert!(!c.subject.is_empty(), "subject must not be empty");
        assert!(
            !c.subject.contains('\n'),
            "subject must be one line: {:?}",
            c.subject
        );
    }
}
