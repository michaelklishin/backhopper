// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `ResolvedPatchInput` against real repos: every `MergePolicy` and
//! `PrCommitPolicy` combination, all parent-count shapes, and the
//! exact error messages users see.

use std::num::NonZeroU32;
use std::path::Path;

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::pr_commit::PrCommitKind;
use backhopper_git::{
    CommitDiffSource, GitRepo, MergePolicy, PatchInputError, PrCommitPolicy, ResolvedPatchInput,
};

use crate::helpers::repo::FakeRepo;

fn sha(raw: &str) -> CommitSha {
    CommitSha::new(raw.to_owned()).unwrap()
}

/// One plain commit on top of the root: `src/a.erl` changes.
fn repo_with_plain_commit() -> (FakeRepo, CommitSha) {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n");
    repo.commit("base");
    repo.write_file(
        "src/a.erl",
        "-module(a).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n",
    );
    repo.commit("add g/0");
    let head = repo.head_sha();
    (repo, sha(&head))
}

/// A `--no-ff` merge of a 2-commit feature branch (second commit is a
/// fixup of the first).
fn repo_with_two_parent_merge() -> (FakeRepo, CommitSha) {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n");
    repo.commit("base");
    repo.checkout_new_branch("feature");
    repo.write_file(
        "src/a.erl",
        "-module(a).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> ok.\n",
    );
    repo.write_file("src/a_helper.erl", "-module(a_helper).\n");
    repo.commit("add g/0");
    repo.write_file(
        "src/a.erl",
        "-module(a).\n-export([f/0, g/0]).\nf() -> ok.\ng() -> done.\n",
    );
    repo.commit("fixup! add g/0");
    repo.checkout("main");
    repo.merge_no_ff("feature", "Merge branch 'feature'");
    let head = repo.head_sha();
    (repo, sha(&head))
}

#[test]
fn plain_commit_resolves_with_parent_diff_base() {
    let (repo, head) = repo_with_plain_commit();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input = ResolvedPatchInput::for_commit(
        &g,
        &head,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    assert_eq!(input.source, CommitDiffSource::Plain);
    assert_eq!(input.source.parent_count(), NonZeroU32::MIN);
    assert!(!input.source.is_merge());
    assert_eq!(input.pr_commits, None);
    let text = String::from_utf8(input.bytes.clone()).unwrap();
    assert!(text.contains("+g() -> ok."), "diff body present: {text}");
    assert_eq!(&input.diff_base, &g.parent_commit(&head).unwrap().unwrap());
}

#[test]
fn two_parent_merge_first_parent_diff_collects_pr_commits() {
    let (repo, merge) = repo_with_two_parent_merge();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input = ResolvedPatchInput::for_commit(
        &g,
        &merge,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    assert!(input.source.is_merge());
    assert_eq!(input.source.parent_count().get(), 2);
    let pr_commits = input.pr_commits.as_ref().expect("2-parent merge collects");
    assert_eq!(pr_commits.len(), 2);
    assert_eq!(pr_commits[0].subject, "add g/0");
    assert_eq!(pr_commits[1].kind, PrCommitKind::Fixup);
    let text = String::from_utf8(input.bytes.clone()).unwrap();
    assert!(text.contains("+g() -> done."), "merge diff present: {text}");
}

#[test]
fn two_parent_merge_skip_policy_leaves_pr_commits_none() {
    let (repo, merge) = repo_with_two_parent_merge();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input = ResolvedPatchInput::for_commit(
        &g,
        &merge,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Skip,
    )
    .unwrap();
    assert!(input.source.is_merge());
    assert_eq!(input.pr_commits, None);
}

#[test]
fn merge_refused_message_is_byte_identical_to_check_commit_contract() {
    let (repo, merge) = repo_with_two_parent_merge();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let err = ResolvedPatchInput::for_commit(&g, &merge, MergePolicy::Refuse, PrCommitPolicy::Skip)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "{merge} is a merge commit (2 parents); use 'backhopper check merge {merge}' instead"
        )
    );
}

#[test]
fn require_merge_rejects_plain_commit_with_parent_count() {
    let (repo, head) = repo_with_plain_commit();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let err = ResolvedPatchInput::for_commit(
        &g,
        &head,
        MergePolicy::RequireMerge,
        PrCommitPolicy::Collect,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("{head} is not a merge commit (parents: 1)")
    );
}

#[test]
fn root_commit_is_rejected_under_every_policy() {
    let repo = FakeRepo::new();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let root = sha(&repo.head_sha());
    for merges in [
        MergePolicy::Refuse,
        MergePolicy::RequireMerge,
        MergePolicy::FirstParentDiff,
    ] {
        let err =
            ResolvedPatchInput::for_commit(&g, &root, merges, PrCommitPolicy::Skip).unwrap_err();
        assert!(matches!(err, PatchInputError::RootCommit(_)));
        assert_eq!(err.to_string(), format!("commit {root} has no parent"));
    }
}

#[test]
fn octopus_merge_takes_first_parent_diff_with_no_pr_commits() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.commit("base");
    repo.checkout_new_branch("b1");
    repo.write_file("src/b1.erl", "-module(b1).\n");
    repo.commit("b1 work");
    repo.checkout("main");
    repo.checkout_new_branch("b2");
    repo.write_file("src/b2.erl", "-module(b2).\n");
    repo.commit("b2 work");
    repo.checkout("main");
    repo.write_file("src/m.erl", "-module(m).\n");
    repo.commit("main work");
    repo.merge_octopus("b1", "b2", "octopus");
    let merge = sha(&repo.head_sha());
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input = ResolvedPatchInput::for_commit(
        &g,
        &merge,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    assert_eq!(input.source.parent_count().get(), 3);
    assert_eq!(input.pr_commits, None);
    let text = String::from_utf8(input.bytes.clone()).unwrap();
    assert!(text.contains("b1.erl") && text.contains("b2.erl"));
}

#[test]
fn ours_merge_yields_empty_patch_bytes() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.commit("base");
    repo.checkout_new_branch("dropped");
    repo.write_file("src/a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n");
    repo.commit("dropped work");
    repo.checkout("main");
    repo.merge_ours("dropped", "Merge branch 'dropped' (dropped)");
    let merge = sha(&repo.head_sha());
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input = ResolvedPatchInput::for_commit(
        &g,
        &merge,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    assert!(input.source.is_merge());
    assert!(input.bytes.is_empty(), "ours-merge diff must be empty");
}

#[test]
fn from_parents_matches_for_commit() {
    let (repo, merge) = repo_with_two_parent_merge();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let parents = g.parents(&merge).unwrap();
    let probed = ResolvedPatchInput::from_parents(
        &g,
        &merge,
        &parents,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    let direct = ResolvedPatchInput::for_commit(
        &g,
        &merge,
        MergePolicy::FirstParentDiff,
        PrCommitPolicy::Collect,
    )
    .unwrap();
    assert_eq!(probed.bytes, direct.bytes);
    assert_eq!(probed.source, direct.source);
    assert_eq!(probed.pr_commits, direct.pr_commits);
    assert_eq!(probed.diff_base, direct.diff_base);
}

#[test]
fn load_source_files_reads_erl_and_hrl_at_diff_base() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.write_file("include/h.hrl", "-define(H, 1).\n");
    repo.write_file("docs/readme.md", "prose\n");
    repo.commit("base");
    repo.write_file("src/a.erl", "-module(a).\n-export([f/0]).\nf() -> ok.\n");
    repo.commit("change");
    let head = sha(&repo.head_sha());
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let input =
        ResolvedPatchInput::for_commit(&g, &head, MergePolicy::Refuse, PrCommitPolicy::Skip)
            .unwrap();
    let files = input.load_source_files(&g).unwrap();
    assert!(files.get(Path::new("src/a.erl")).is_some());
    assert!(files.get(Path::new("include/h.hrl")).is_some());
    assert!(files.get(Path::new("docs/readme.md")).is_none());
}
