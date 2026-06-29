// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Walk-correctness fixture for `pr_commits_for`. Builds a tiny repo
//! with a known 2-parent merge structure and asserts the walk
//! enumerates exactly the PR-branch commits.

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::pr_commit::PrCommitKind;
use backhopper_git::{GitRepo, pr_commits_for};
use backhopper_test_support::GitRepoFixture;

struct Fixture {
    _fixture: GitRepoFixture,
    repo: GitRepo,
}

fn head(fixture: &GitRepoFixture) -> CommitSha {
    CommitSha::new(fixture.head_sha()).unwrap()
}

fn fixture_with_merge() -> (Fixture, CommitSha, Vec<CommitSha>) {
    let fixture = GitRepoFixture::new();
    fixture.write_file("README.md", "init\n");
    fixture.commit("initial");
    fixture.checkout_new_branch("feature");
    fixture.write_file("src/a.txt", "a\n");
    fixture.commit("Add src/a.txt");
    let pr1 = head(&fixture);
    fixture.write_file("src/b.txt", "b\n");
    fixture.commit("Add src/b.txt");
    let pr2 = head(&fixture);
    fixture.write_file("src/a.txt", "a-fixed\n");
    fixture.commit("Resolve conflicts");
    let pr3 = head(&fixture);
    fixture.checkout("main");
    fixture.merge_no_ff("feature", "Merge branch 'feature'");
    let merge = head(&fixture);
    let repo = GitRepo::open(fixture.path()).expect("open");
    (
        Fixture {
            _fixture: fixture,
            repo,
        },
        merge,
        vec![pr1, pr2, pr3],
    )
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
