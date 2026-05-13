use backhopper_core::git::GitRepo;
use backhopper_core::model::names::TagName;

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
    let strings: Vec<_> = tags.iter().map(|t| t.to_string()).collect();
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
