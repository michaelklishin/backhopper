// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_cli::commands::snapshot_cache::SnapshotCache;
use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader, state};
use backhopper_core::store::SnapshotStore;

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
    }
}

fn write_snapshot(root: &std::path::Path, project: &str, tag: &str) {
    let store = SnapshotStore::open_mut(root).unwrap();
    let snap =
        Snapshot::<state::Unsorted>::from_extracted(header(project, tag), Vec::new(), Vec::new())
            .into_canonical();
    store.write(&snap).unwrap();
}

#[test]
fn cache_returns_same_arc_on_second_lookup() {
    let tmp = TempDir::new().unwrap();
    write_snapshot(tmp.path(), "ra", "v3.1.7");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let cache = SnapshotCache::new(&store);
    let project = ProjectName::new("ra").unwrap();
    let tag = TagName::new("v3.1.7").unwrap();
    let first = cache.get(&project, &tag).unwrap();
    let second = cache.get(&project, &tag).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
}

#[test]
fn cache_distinct_pins_get_distinct_entries() {
    let tmp = TempDir::new().unwrap();
    write_snapshot(tmp.path(), "ra", "v3.1.7");
    write_snapshot(tmp.path(), "ra", "v3.1.8");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let cache = SnapshotCache::new(&store);
    let project = ProjectName::new("ra").unwrap();
    let v17 = TagName::new("v3.1.7").unwrap();
    let v18 = TagName::new("v3.1.8").unwrap();
    let a = cache.get(&project, &v17).unwrap();
    let b = cache.get(&project, &v18).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&a, &b));
    assert_eq!(cache.len(), 2);
}

#[test]
fn cache_propagates_store_errors() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let cache = SnapshotCache::new(&store);
    let project = ProjectName::new("nonexistent").unwrap();
    let tag = TagName::new("v0.0.0").unwrap();
    assert!(cache.get(&project, &tag).is_err());
    assert!(cache.is_empty());
}
