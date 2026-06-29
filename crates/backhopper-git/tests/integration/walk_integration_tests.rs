// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::fmt::Write;

use backhopper_core::model::names::CommitSha;
use backhopper_git::walk::{first_parent_walk_since, patch_id};
use backhopper_git::{GitRepo, cherry_pick_trailers};

use backhopper_test_support::GitRepoFixture;

fn open(fake: &GitRepoFixture) -> GitRepo {
    GitRepo::open(fake.dir.path().to_path_buf()).unwrap()
}

fn sha(raw: &str) -> CommitSha {
    raw.parse().unwrap()
}

#[test]
fn same_branch_window_is_pure_ancestry_newest_first() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    let since = fake.head_sha();
    fake.write_file("src/a.erl", "a2\n");
    fake.commit("first after");
    let first = fake.head_sha();
    fake.write_file("src/a.erl", "a3\n");
    fake.commit("second after");
    let second = fake.head_sha();

    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&second), &sha(&since)).unwrap();
    let shas: Vec<&str> = window.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(shas, vec![second.as_str(), first.as_str()]);
}

// On the same branch the window is pure ancestry: an intermediate commit
// dated before `since` (and even before the committer-time cutoff) must still
// appear, since the cutoff only bounds the cross-branch case.
#[test]
fn same_branch_walk_keeps_intermediate_commit_dated_before_the_since_point() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit_dated("base", "2025-01-01T10:00:00Z");
    let since = fake.head_sha();
    fake.write_file("src/a.erl", "a2\n");
    fake.commit_dated("old intermediate", "2020-01-01T10:00:00Z");
    let mid = fake.head_sha();
    fake.write_file("src/a.erl", "a3\n");
    fake.commit_dated("tip", "2025-06-10T10:00:00Z");
    let tip = fake.head_sha();

    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&tip), &sha(&since)).unwrap();
    let shas: Vec<&str> = window.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(shas, vec![tip.as_str(), mid.as_str()]);
}

#[test]
fn since_equal_to_tip_gives_an_empty_window() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("only");
    let tip = fake.head_sha();
    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&tip), &sha(&tip)).unwrap();
    assert!(window.is_empty());
}

#[test]
fn cross_branch_window_is_bounded_by_committer_time() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit_dated("shared base", "2025-01-01T10:00:00Z");
    // the since-point lives on a stable branch, not on main
    fake.checkout_new_branch("v1.x");
    fake.write_file("src/r.erl", "r\n");
    fake.commit_dated("release prep", "2025-03-01T10:00:00Z");
    let since = fake.head_sha();
    fake.checkout("main");
    fake.write_file("src/a.erl", "old\n");
    fake.commit_dated("older than the bound", "2025-01-15T10:00:00Z");
    fake.write_file("src/a.erl", "new\n");
    fake.commit_dated("newer than the bound", "2025-04-01T10:00:00Z");
    let newer = fake.head_sha();

    let repo = open(&fake);
    let tip = fake.head_sha();
    let window = first_parent_walk_since(&repo, &sha(&tip), &sha(&since)).unwrap();
    let shas: Vec<&str> = window.iter().map(|c| c.sha.as_str()).collect();
    assert_eq!(shas, vec![newer.as_str()]);
}

#[test]
fn source_branch_behind_the_since_point_gives_an_empty_window() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit_dated("old main tip", "2025-01-01T10:00:00Z");
    let main_tip = fake.head_sha();
    fake.checkout_new_branch("v1.x");
    fake.write_file("src/r.erl", "r\n");
    fake.commit_dated("much newer release", "2025-06-01T10:00:00Z");
    let since = fake.head_sha();

    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&main_tip), &sha(&since)).unwrap();
    assert!(window.is_empty());
}

#[test]
fn a_merge_surfaces_once_with_both_parents_and_inner_commits_stay_hidden() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    let since = fake.head_sha();
    fake.checkout_new_branch("pr");
    fake.write_file("src/a.erl", "a2\n");
    fake.commit("inner pr commit");
    let inner = fake.head_sha();
    fake.checkout("main");
    // advance main so the merge cannot fast-forward
    fake.write_file("src/b.erl", "b\n");
    fake.commit("mainline work");
    fake.merge_no_ff("pr", "Merge pull request #1 from org/pr");
    let merge = fake.head_sha();

    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&merge), &sha(&since)).unwrap();
    let shas: Vec<&str> = window.iter().map(|c| c.sha.as_str()).collect();
    assert!(shas.contains(&merge.as_str()));
    assert!(!shas.contains(&inner.as_str()));
    let merge_entry = window.iter().find(|c| c.sha.as_str() == merge).unwrap();
    assert_eq!(merge_entry.parents.len(), 2);
}

#[test]
fn walked_commits_carry_subject_and_full_message() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    let since = fake.head_sha();
    fake.write_file("src/a.erl", "a2\n");
    fake.commit("subject line\n\nbody first line\n(cherry picked from commit 0123456789abcdef0123456789abcdef01234567)");
    let tip = fake.head_sha();

    let repo = open(&fake);
    let window = first_parent_walk_since(&repo, &sha(&tip), &sha(&since)).unwrap();
    assert_eq!(window.len(), 1);
    assert_eq!(window[0].subject, "subject line");
    assert_eq!(cherry_pick_trailers(&window[0].message).len(), 1);
}

#[test]
fn commit_timestamp_returns_the_committer_time() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit_dated("dated", "2025-05-01T12:00:00Z");
    let tip = fake.head_sha();
    let repo = open(&fake);
    let ts = repo.commit_timestamp(&sha(&tip)).unwrap();
    assert_eq!(ts.unix_timestamp(), 1_746_100_800);
}

#[test]
fn is_ancestor_handles_self_forward_and_unrelated_cases() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("side");
    fake.write_file("src/a.erl", "side\n");
    fake.commit("side work");
    let side = fake.head_sha();
    fake.checkout("main");
    fake.write_file("src/a.erl", "main\n");
    fake.commit("main work");
    let main_tip = fake.head_sha();

    let repo = open(&fake);
    assert!(repo.is_ancestor(&sha(&base), &sha(&main_tip)).unwrap());
    assert!(repo.is_ancestor(&sha(&base), &sha(&base)).unwrap());
    assert!(!repo.is_ancestor(&sha(&main_tip), &sha(&base)).unwrap());
    assert!(!repo.is_ancestor(&sha(&side), &sha(&main_tip)).unwrap());
}

#[test]
fn ancestors_among_screens_many_candidates_in_one_walk() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    let base = fake.head_sha();
    fake.checkout_new_branch("side");
    fake.write_file("src/a.erl", "side\n");
    fake.commit("side work");
    let side = fake.head_sha();
    fake.checkout("main");
    fake.write_file("src/a.erl", "main\n");
    fake.commit("main work");
    let main_tip = fake.head_sha();

    let repo = open(&fake);
    let candidates: BTreeSet<CommitSha> = [sha(&base), sha(&side), sha(&main_tip)].into();
    let reachable = repo.ancestors_among(&sha(&main_tip), &candidates).unwrap();
    assert!(reachable.contains(&sha(&base)));
    assert!(reachable.contains(&sha(&main_tip)));
    assert!(!reachable.contains(&sha(&side)));

    let empty = repo
        .ancestors_among(&sha(&main_tip), &BTreeSet::new())
        .unwrap();
    assert!(empty.is_empty());
}

#[test]
fn local_branch_tips_lists_branches_sorted_with_tips() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    fake.checkout_new_branch("v1.x");
    fake.write_file("src/a.erl", "v1\n");
    fake.commit("v1 work");
    let v1_tip = fake.head_sha();
    fake.checkout("main");

    let repo = open(&fake);
    let tips = repo.local_branch_tips().unwrap();
    let names: Vec<&str> = tips.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["main", "v1.x"]);
    let v1 = tips.iter().find(|(n, _)| n == "v1.x").unwrap();
    assert_eq!(v1.1.as_str(), v1_tip);
}

#[test]
fn patch_id_survives_a_cherry_pick_at_a_different_offset() {
    let fake = GitRepoFixture::new();
    let padding: String = (1..=12).fold(String::new(), |mut acc, i| {
        let _ = writeln!(acc, "line{i}");
        acc
    });
    fake.write_file("src/a.erl", &format!("{padding}target\nend\n"));
    fake.commit("base");
    fake.checkout_new_branch("v1.x");
    fake.checkout("main");
    // change the target line on main
    fake.write_file("src/a.erl", &format!("{padding}changed\nend\n"));
    fake.commit("the fix");
    let fix = fake.head_sha();
    // shift the same region down on v1.x first, then pick the fix
    fake.checkout("v1.x");
    fake.write_file(
        "src/a.erl",
        &format!("extra1\nextra2\nextra3\nextra4\n{padding}target\nend\n"),
    );
    fake.commit("unrelated prelude");
    fake.cherry_pick_x(&fix);
    let pick = fake.head_sha();

    let repo = open(&fake);
    let fix_parent = repo.parent_commit(&sha(&fix)).unwrap().unwrap();
    let pick_parent = repo.parent_commit(&sha(&pick)).unwrap().unwrap();
    let original = patch_id(&repo, &fix_parent, &sha(&fix)).unwrap().unwrap();
    let picked = patch_id(&repo, &pick_parent, &sha(&pick)).unwrap().unwrap();
    assert_eq!(original, picked);
}

#[test]
fn patch_id_is_none_for_commits_with_no_analysable_diff() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    fake.write_file("README.md", "docs only\n");
    fake.commit("docs change");
    let docs = fake.head_sha();
    let repo = open(&fake);
    let parent = repo.parent_commit(&sha(&docs)).unwrap().unwrap();
    assert!(patch_id(&repo, &parent, &sha(&docs)).unwrap().is_none());
}

#[test]
fn different_changes_get_different_patch_ids() {
    let fake = GitRepoFixture::new();
    fake.write_file("src/a.erl", "a1\n");
    fake.commit("base");
    fake.write_file("src/a.erl", "a2\n");
    fake.commit("first change");
    let first = fake.head_sha();
    fake.write_file("src/a.erl", "a3\n");
    fake.commit("second change");
    let second = fake.head_sha();
    let repo = open(&fake);
    let p1 = repo.parent_commit(&sha(&first)).unwrap().unwrap();
    let p2 = repo.parent_commit(&sha(&second)).unwrap().unwrap();
    let id1 = patch_id(&repo, &p1, &sha(&first)).unwrap().unwrap();
    let id2 = patch_id(&repo, &p2, &sha(&second)).unwrap().unwrap();
    assert_ne!(id1, id2);
}
