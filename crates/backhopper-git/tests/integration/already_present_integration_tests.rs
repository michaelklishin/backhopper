// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::model::names::CommitSha;
use backhopper_core::model::verdict::TargetMatchKind;
use backhopper_git::walk::patch_id;
use backhopper_git::{CandidateIdentity, GitRepo, TargetWalkIndex, trailer_origin_on_target};

use backhopper_test_support::GitRepoFixture;

fn open(fake: &GitRepoFixture) -> GitRepo {
    GitRepo::open(fake.dir.path().to_path_buf()).unwrap()
}

fn sha(raw: &str) -> CommitSha {
    raw.parse().unwrap()
}

fn candidate(commit: &str) -> CandidateIdentity {
    CandidateIdentity {
        sha: sha(commit),
        trailer_origins: Vec::new(),
        inner_pr_shas: Vec::new(),
        patch_hash: None,
        touched_paths: Vec::new(),
    }
}

/// main carries the candidate; v1 carries a `-x` pick of it. The
/// walked pick's trailer names the candidate.
#[test]
fn x_pick_on_target_matches_via_trailer_intersection() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "base\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("v1");
    fake.checkout("main");
    fake.write_file("src/a.erl", "feature\n");
    fake.commit("feature work");
    let cand = fake.head_sha();
    fake.checkout("v1");
    fake.cherry_pick_x(&cand);
    let tip = fake.head_sha();

    let repo = open(&fake);
    let index = TargetWalkIndex::build(&repo, &sha(&tip), &sha(&base), 5000).unwrap();
    let m = index.find_match(&repo, &candidate(&cand)).unwrap();
    assert_eq!(m.via, TargetMatchKind::TrailerIntersection);
    assert_eq!(m.commit.as_str(), tip);
    assert_eq!(m.subject, "feature work");
}

/// The target pick's trailer names an inner commit of the candidate
/// merge, not the candidate itself.
#[test]
fn inner_pr_sha_named_by_target_trailer_matches() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "base\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("v1");
    fake.checkout("main");
    fake.write_file("src/a.erl", "inner\n");
    fake.commit("inner work");
    let inner = fake.head_sha();
    fake.checkout("v1");
    fake.cherry_pick_x(&inner);
    let tip = fake.head_sha();

    let repo = open(&fake);
    let index = TargetWalkIndex::build(&repo, &sha(&tip), &sha(&base), 5000).unwrap();
    // the candidate is some merge whose pr_commits include inner
    let mut cand = candidate(&base);
    cand.inner_pr_shas = vec![sha(&inner)];
    let m = index.find_match(&repo, &cand).unwrap();
    assert_eq!(m.via, TargetMatchKind::TrailerIntersection);
}

/// A hand-landed identical change (no trailer) is found by the
/// normalized patch hash, gated by the touched-path pre-filter.
#[test]
fn hand_landed_identical_patch_matches_via_patch_id() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "line1\nline2\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("v1");
    fake.checkout("main");
    fake.write_file("src/a.erl", "line1\nfixed\n");
    fake.commit("fix on main");
    let cand = fake.head_sha();
    fake.checkout("v1");
    fake.write_file("src/a.erl", "line1\nfixed\n");
    fake.commit("same fix, landed by hand");
    let tip = fake.head_sha();

    let repo = open(&fake);
    let cand_hash = patch_id(&repo, &sha(&base), &sha(&cand))
        .unwrap()
        .map(|p| p.as_str().to_owned());
    assert!(cand_hash.is_some());
    let index = TargetWalkIndex::build(&repo, &sha(&tip), &sha(&base), 5000).unwrap();
    let mut c = candidate(&cand);
    c.patch_hash = cand_hash;
    c.touched_paths = vec![PathBuf::from("src/a.erl")];
    let m = index.find_match(&repo, &c).unwrap();
    assert_eq!(m.via, TargetMatchKind::PatchId);
    assert_eq!(m.subject, "same fix, landed by hand");
}

/// A different change to the same file matches neither trailers nor
/// patch hash: tier 1 reports nothing and tier 2 handles the case.
#[test]
fn different_refactor_matches_nothing() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "line1\nline2\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("v1");
    fake.checkout("main");
    fake.write_file("src/a.erl", "line1\nfixed\n");
    fake.commit("fix on main");
    let cand = fake.head_sha();
    fake.checkout("v1");
    fake.write_file("src/a.erl", "differently fixed\nline2\n");
    fake.commit("equivalent refactor");
    let tip = fake.head_sha();

    let repo = open(&fake);
    let cand_hash = patch_id(&repo, &sha(&base), &sha(&cand))
        .unwrap()
        .map(|p| p.as_str().to_owned());
    let index = TargetWalkIndex::build(&repo, &sha(&tip), &sha(&base), 5000).unwrap();
    let mut c = candidate(&cand);
    c.patch_hash = cand_hash;
    c.touched_paths = vec![PathBuf::from("src/a.erl")];
    assert!(index.find_match(&repo, &c).is_none());
}

/// The candidate's own trailer names an origin that is an ancestor of
/// the target tip: caught by the ancestry tier, no walk involved.
#[test]
fn candidate_trailer_origin_ancestral_to_target_matches() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "origin work\n");
    fake.commit("origin work");
    let origin = fake.head_sha();
    fake.write_file("src/b.erl", "more\n");
    fake.commit("target tip moves on");
    let tip = fake.head_sha();

    let repo = open(&fake);
    let m = trailer_origin_on_target(&repo, &[sha(&origin)], &sha(&tip))
        .unwrap()
        .unwrap();
    assert_eq!(m.via, TargetMatchKind::TrailerAncestry);
    assert_eq!(m.commit.as_str(), origin);
    assert_eq!(m.subject, "origin work");
}

/// Origins absent from the target object store are skipped without
/// erroring.
#[test]
fn missing_trailer_origin_is_skipped_not_an_error() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "x\n");
    fake.commit("only");
    let tip = fake.head_sha();
    let repo = open(&fake);
    let missing = sha(&"f".repeat(40));
    let m = trailer_origin_on_target(&repo, &[missing], &sha(&tip)).unwrap();
    assert!(m.is_none());
}

#[test]
fn walk_limit_truncates_and_reports_it() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "0\n");
    fake.commit("base");
    let base = fake.head_sha();
    for i in 1..=3 {
        fake.write_file("src/a.erl", &format!("{i}\n"));
        fake.commit(&format!("change {i}"));
    }
    let tip = fake.head_sha();
    let repo = open(&fake);
    let index = TargetWalkIndex::build(&repo, &sha(&tip), &sha(&base), 2).unwrap();
    assert!(index.truncated);
    assert_eq!(index.len(), 2);
}

#[test]
fn merge_base_of_unrelated_commits_is_none_and_missing_object_errors() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/ra_server.erl", "x\n");
    fake.commit("only");
    let tip = fake.head_sha();
    let repo = open(&fake);
    let missing = sha(&"e".repeat(40));
    assert!(repo.merge_base(&missing, &sha(&tip)).is_err());
    assert!(repo.has_commit(&sha(&tip)).unwrap());
    assert!(!repo.has_commit(&missing).unwrap());
}

#[test]
fn merge_base_returns_the_common_ancestor() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/ra_log.erl", "base\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("v1");
    fake.write_file("src/ra_log.erl", "stable change\n");
    fake.commit("stable work");
    let v1_tip = fake.head_sha();
    fake.checkout("main");
    fake.write_file("src/ra_log.erl", "main change\n");
    fake.commit("main work");
    let main_tip = fake.head_sha();

    let repo = open(&fake);
    let merge_base = repo.merge_base(&sha(&main_tip), &sha(&v1_tip)).unwrap();
    assert_eq!(merge_base, Some(sha(&base)));
}

#[test]
fn has_commit_is_false_for_tree_and_blob_objects() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/ra_machine.erl", "x\n");
    fake.commit("only");
    let tree = fake.rev_parse("HEAD^{tree}");
    let blob = fake.rev_parse("HEAD:src/ra_machine.erl");
    let repo = open(&fake);
    assert!(
        !repo.has_commit(&sha(&tree)).unwrap(),
        "a tree is not a commit"
    );
    assert!(
        !repo.has_commit(&sha(&blob)).unwrap(),
        "a blob is not a commit"
    );
}
