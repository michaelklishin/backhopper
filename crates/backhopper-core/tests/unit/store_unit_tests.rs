// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::env;
use std::panic;
use std::path::PathBuf;

use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader, state};
use backhopper_core::store::SnapshotStore;

fn make_snapshot(project: &str, tag: &str) -> Snapshot<state::Canonical> {
    let header = SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "backhopper 0.1.0".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    };
    let mut m = Module::new(ModuleName::new("foo").unwrap());
    m.exports.push(FunArity {
        name: FunctionName::new("bar").unwrap(),
        arity: Arity::new(1),
    });
    Snapshot::from_extracted(header, vec![m], vec![]).into_canonical()
}

#[test]
fn store_open_mut_creates_root_directory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("snapshots");
    SnapshotStore::open_mut(&root).unwrap();
    assert!(root.exists());
}

#[test]
fn store_write_and_read_round_trip() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = make_snapshot("ra", "v3.1.6");
    store.write(&snap).unwrap();
    let project = ProjectName::new("ra").unwrap();
    let tag = TagName::new("v3.1.6").unwrap();
    let read = store.read(&project, &tag).unwrap();
    assert_eq!(snap, read);
}

#[test]
fn store_list_tags_returns_captured() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    store.write(&make_snapshot("ra", "v3.1.6")).unwrap();
    store.write(&make_snapshot("ra", "v3.1.5")).unwrap();
    let project = ProjectName::new("ra").unwrap();
    let tags = store.list_tags(&project).unwrap();
    assert_eq!(
        tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
        vec!["v3.1.5", "v3.1.6"]
    );
}

#[test]
fn store_has_returns_false_for_missing() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let project = ProjectName::new("ra").unwrap();
    let tag = TagName::new("v0.0.0").unwrap();
    assert!(!store.has(&project, &tag));
}

#[test]
fn store_read_open_returns_not_found() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let project = ProjectName::new("ra").unwrap();
    let tag = TagName::new("v3.1.6").unwrap();
    let r = store.read(&project, &tag);
    assert!(r.is_err());
}

#[test]
fn store_with_relative_root_round_trips() {
    let tmp = TempDir::new().unwrap();
    let original = env::current_dir().unwrap();
    env::set_current_dir(tmp.path()).unwrap();
    let result = panic::catch_unwind(|| {
        let store = SnapshotStore::open_mut(PathBuf::from("./snapshots")).unwrap();
        let snap = make_snapshot("ra", "v3.1.6");
        store.write(&snap).unwrap();
        let project = ProjectName::new("ra").unwrap();
        let tag = TagName::new("v3.1.6").unwrap();
        let read = store.read(&project, &tag).unwrap();
        assert_eq!(snap, read);
    });
    env::set_current_dir(original).unwrap();
    result.unwrap();
}

#[test]
fn store_delete_removes_snapshot() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(tmp.path()).unwrap();
    let snap = make_snapshot("ra", "v3.1.6");
    store.write(&snap).unwrap();
    let project = ProjectName::new("ra").unwrap();
    let tag = TagName::new("v3.1.6").unwrap();
    assert!(store.has(&project, &tag));
    store.delete(&project, &tag).unwrap();
    assert!(!store.has(&project, &tag));
}
