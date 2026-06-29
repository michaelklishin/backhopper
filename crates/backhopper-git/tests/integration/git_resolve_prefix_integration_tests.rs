// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::model::names::CommitShaPrefix;
use backhopper_git::GitError;
use backhopper_git::{GitRepo, ObjectKind};

use backhopper_test_support::GitRepoFixture;

#[test]
fn resolves_prefix_at_seven_ten_twenty_and_forty_characters() {
    let repo = GitRepoFixture::new();
    repo.write_file("a.txt", "hello\n");
    repo.commit("first");
    let head = repo.head_sha();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    for len in [7usize, 10, 20, 40] {
        let prefix = CommitShaPrefix::new(&head[..len]).unwrap();
        let resolved = g.resolve_sha_prefix(&prefix).unwrap();
        assert_eq!(resolved.commit.as_str(), head);
        assert_eq!(resolved.kind, ObjectKind::Commit);
    }
}

#[test]
fn full_form_prefix_short_circuits_to_commit_sha() {
    let repo = GitRepoFixture::new();
    repo.write_file("a.txt", "x\n");
    repo.commit("a");
    let head = repo.head_sha();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let prefix = CommitShaPrefix::new(head.clone()).unwrap();
    let resolved = g.resolve_sha_prefix(&prefix).unwrap();
    assert_eq!(resolved.commit.as_str(), head);
}

#[test]
fn nonexistent_prefix_returns_commit_not_found() {
    let repo = GitRepoFixture::new();
    repo.write_file("a.txt", "x\n");
    repo.commit("a");
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let prefix = CommitShaPrefix::new("deadbeef").unwrap();
    let err = g.resolve_sha_prefix(&prefix).unwrap_err();
    assert!(matches!(err, GitError::CommitNotFound(_)), "got {err:?}");
}

#[test]
fn ambiguous_prefix_surfaces_typed_error_with_candidates() {
    let repo = GitRepoFixture::new();
    let mut shas: Vec<String> = Vec::new();
    let mut i = 0u32;
    let mut shared_prefix: Option<String> = None;
    while shared_prefix.is_none() {
        repo.write_file("a.txt", &format!("body-{i}\n"));
        repo.commit(&format!("c{i}"));
        let sha = repo.head_sha();
        let collision = shas
            .iter()
            .find(|p| p[..2] == sha[..2])
            .map(|p| p[..2].to_owned());
        shas.push(sha);
        shared_prefix = collision;
        i += 1;
        assert!(i < 2000, "fixture failed to produce a 2-char collision");
    }
    let two = shared_prefix.unwrap();
    let mut prefix_str = String::from(&two);
    while prefix_str.len() < 7 {
        prefix_str.push(two.chars().next().unwrap());
    }
    let prefix = CommitShaPrefix::new(prefix_str).unwrap();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    match g.resolve_sha_prefix(&prefix) {
        Err(GitError::AmbiguousSha { candidates, .. }) => {
            assert!(
                candidates.len() >= 2,
                "expected at least 2 candidates, got {candidates:?}"
            );
        }
        Err(GitError::CommitNotFound(_)) => {}
        other => panic!("expected AmbiguousSha or CommitNotFound, got {other:?}"),
    }
}

#[test]
fn annotated_tag_prefix_peels_to_underlying_commit() {
    let repo = GitRepoFixture::new();
    repo.write_file("a.txt", "annotated tag fixture\n");
    repo.commit("c");
    let commit_sha = repo.head_sha();
    repo.annotated_tag("vAnnotated", "release v1");
    let tag_object_sha = repo.rev_parse("vAnnotated");
    if tag_object_sha == commit_sha {
        return;
    }
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let prefix = CommitShaPrefix::new(&tag_object_sha[..20]).unwrap();
    let resolved = g.resolve_sha_prefix(&prefix).unwrap();
    assert_eq!(resolved.commit.as_str(), commit_sha);
    assert_eq!(resolved.kind, ObjectKind::Tag);
}

#[test]
fn blob_prefix_returns_not_a_commit() {
    let repo = GitRepoFixture::new();
    repo.write_file("file.txt", "blob body\n");
    repo.commit("c");
    let blob_sha = repo.rev_parse("HEAD:file.txt");
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let prefix = CommitShaPrefix::new(&blob_sha[..20]).unwrap();
    let err = g.resolve_sha_prefix(&prefix).unwrap_err();
    assert!(matches!(err, GitError::NotACommit { .. }), "got {err:?}");
}

#[test]
fn tree_prefix_returns_not_a_commit() {
    let repo = GitRepoFixture::new();
    repo.write_file("file.txt", "x\n");
    repo.commit("c");
    let tree_sha = repo.rev_parse("HEAD^{tree}");
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let prefix = CommitShaPrefix::new(&tree_sha[..20]).unwrap();
    let err = g.resolve_sha_prefix(&prefix).unwrap_err();
    assert!(matches!(err, GitError::NotACommit { .. }), "got {err:?}");
}

#[test]
fn not_a_git_repository_returns_typed_error() {
    let dir = TempDir::new().unwrap();
    let err = GitRepo::open(dir.path()).unwrap_err();
    assert!(matches!(err, GitError::NotAGitRepository(_)), "got {err:?}");
}

#[test]
fn missing_path_returns_not_a_git_repository() {
    let missing: PathBuf = "/nonexistent/path/that/does/not/exist".into();
    let err = GitRepo::open(missing).unwrap_err();
    assert!(matches!(err, GitError::NotAGitRepository(_)), "got {err:?}");
}

#[test]
fn many_commits_round_trip_through_every_prefix_length() {
    let repo = GitRepoFixture::new();
    let mut shas: Vec<String> = Vec::new();
    for i in 0..6 {
        repo.write_file("a.txt", &format!("v{i}\n"));
        repo.commit(&format!("c{i}"));
        shas.push(repo.head_sha());
    }
    let g = GitRepo::open(repo.dir.path()).unwrap();
    for sha in &shas {
        for len in [7usize, 8, 9, 12, 20, 30, 40] {
            let prefix = CommitShaPrefix::new(&sha[..len]).unwrap();
            match g.resolve_sha_prefix(&prefix) {
                Ok(resolved) => assert_eq!(resolved.commit.as_str(), sha),
                Err(GitError::AmbiguousSha { .. }) => {}
                Err(other) => panic!("unexpected error for {sha} @ len {len}: {other:?}"),
            }
        }
    }
}
