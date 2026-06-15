// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::TagName;
use backhopper_git::GitRepo;

use crate::helpers::repo::FakeRepo;

#[test]
fn list_tags_returns_known_tags() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.commit("a");
    repo.tag("v1.0.0");
    repo.write_file("src/a.erl", "-module(a).\n-export([f/1]).\nf(X) -> X.\n");
    repo.commit("b");
    repo.tag("v1.1.0");

    let g = GitRepo::open(repo.dir.path()).unwrap();
    let tags = g.list_tags().unwrap();
    let strings: Vec<_> = tags.iter().map(|s| s.to_string()).collect();
    assert!(strings.iter().any(|s| s == "v1.0.0"));
    assert!(strings.iter().any(|s| s == "v1.1.0"));
}

#[test]
fn read_paths_at_tag_returns_blobs() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n-export([f/1]).\nf(X) -> X.\n");
    repo.commit("first");
    repo.tag("v1.0.0");

    let g = GitRepo::open(repo.dir.path()).unwrap();
    let tag = TagName::new("v1.0.0").unwrap();
    let blobs = g.read_paths_at_tag(&tag, |p| p.ends_with(".erl")).unwrap();
    assert_eq!(blobs.len(), 1);
    assert!(String::from_utf8_lossy(&blobs[0].bytes).contains("-module(a)"));
}

#[test]
fn resolve_tag_returns_commit_sha() {
    let repo = FakeRepo::new();
    repo.write_file("README.md", "hi\n");
    repo.commit("readme");
    repo.tag("vX");
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let sha = g.resolve_tag(&TagName::new("vX").unwrap()).unwrap();
    assert_eq!(sha.as_str().len(), 40);
}

#[test]
fn missing_tag_yields_error() {
    let repo = FakeRepo::new();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let r = g.resolve_tag(&TagName::new("nope").unwrap());
    assert!(r.is_err());
}

#[test]
fn list_tag_refs_separates_parseable_tags_from_skipped_refs() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.commit("a");
    repo.tag("v1.0.0");
    repo.tag("v1.1.0");
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let listing = g.list_tag_refs().unwrap();
    assert_eq!(listing.tags.len(), 2);
    assert!(
        listing.skipped.is_empty(),
        "no malformed refs expected, got {:?}",
        listing.skipped
    );
}

#[test]
fn diff_commits_unified_covers_added_modified_and_deleted_files() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\nold().\n");
    repo.write_file("src/gone.erl", "-module(gone).\n");
    repo.write_file("src/untouched.erl", "-module(untouched).\n");
    repo.commit("before");
    let from = repo.head_sha();
    repo.write_file("src/a.erl", "-module(a).\nnew().\n");
    repo.write_file("src/added.erl", "-module(added).\n");
    std::fs::remove_file(repo.dir.path().join("src/gone.erl")).unwrap();
    repo.stage_all();
    repo.commit("after");
    let to = repo.head_sha();

    let g = GitRepo::open(repo.dir.path()).unwrap();
    let diff = g
        .diff_commits_unified(&from.parse().unwrap(), &to.parse().unwrap(), |p| {
            p.ends_with(".erl")
        })
        .unwrap();
    assert!(diff.contains("diff --git a/src/a.erl b/src/a.erl"));
    assert!(diff.contains("-old().\n"));
    assert!(diff.contains("+new().\n"));
    assert!(diff.contains("+-module(added)."));
    assert!(diff.contains("--module(gone)."));
    // added and deleted files name /dev/null on the empty side so the
    // core diff parser resolves them as additions and deletions
    assert!(diff.contains("--- /dev/null\n+++ b/src/added.erl"));
    assert!(diff.contains("--- a/src/gone.erl\n+++ /dev/null"));
    // unchanged files never appear
    assert!(!diff.contains("untouched"));
    // emission order: present-side files sorted first, deletions last
    let added_at = diff.find("src/added.erl").unwrap();
    let modified_at = diff.find("src/a.erl").unwrap();
    let deleted_at = diff.find("src/gone.erl").unwrap();
    assert!(modified_at < added_at && added_at < deleted_at);
}

#[test]
fn diff_commits_unified_honours_the_path_filter() {
    let repo = FakeRepo::new();
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.write_file("README.md", "before\n");
    repo.commit("before");
    let from = repo.head_sha();
    repo.write_file("src/a.erl", "-module(a).\n-export([f/0]).\n");
    repo.write_file("README.md", "after\n");
    repo.commit("after");
    let to = repo.head_sha();

    let g = GitRepo::open(repo.dir.path()).unwrap();
    let diff = g
        .diff_commits_unified(&from.parse().unwrap(), &to.parse().unwrap(), |p| {
            p.ends_with(".erl")
        })
        .unwrap();
    assert!(diff.contains("src/a.erl"));
    assert!(!diff.contains("README.md"));
}

#[test]
fn list_paths_at_commit_returns_sorted_paths_without_contents() {
    let repo = FakeRepo::new();
    repo.write_file("src/b.erl", "-module(b).\n");
    repo.write_file("src/a.erl", "-module(a).\n");
    repo.write_file("include/h.hrl", "-define(H, 1).\n");
    repo.commit("tree");
    let head = repo.head_sha();
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let paths = g.list_paths_at_commit(&head.parse().unwrap()).unwrap();
    let strings: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    assert_eq!(strings, ["include/h.hrl", "src/a.erl", "src/b.erl"]);
}
