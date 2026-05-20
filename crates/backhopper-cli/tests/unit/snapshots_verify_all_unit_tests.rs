// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::Path;

use tempfile::tempdir;
use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::snapshot::format;
use backhopper_core::store::SnapshotStore;

fn header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        generated_by: "backhopper".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
    }
}

fn write_canonical(store_root: &Path, project: &str, tag: &str) {
    let snap = Snapshot::from_extracted(header(project, tag), vec![], vec![]).into_canonical();
    let dir = store_root.join(project);
    fs::create_dir_all(&dir).unwrap();
    let text = format::to_string(&snap).unwrap();
    fs::write(dir.join(format!("{tag}.api.txt")), text).unwrap();
}

#[test]
fn list_projects_and_list_tags_walk_a_real_store() {
    let dir = tempdir().unwrap();
    write_canonical(dir.path(), "ra", "v1.0.0");
    write_canonical(dir.path(), "ra", "v1.1.0");
    write_canonical(dir.path(), "osiris", "v2.0.0");
    let store = SnapshotStore::open(dir.path()).unwrap();
    let mut projects = store.list_projects().unwrap();
    projects.sort();
    assert_eq!(
        projects.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        vec!["osiris", "ra"]
    );
    let ra_tags = store.list_tags(&ProjectName::new("ra").unwrap()).unwrap();
    assert_eq!(ra_tags.len(), 2);
}

#[test]
fn valid_snapshot_round_trips_through_store_read() {
    let dir = tempdir().unwrap();
    write_canonical(dir.path(), "ra", "v1.0.0");
    let store = SnapshotStore::open(dir.path()).unwrap();
    let snap = store
        .read(
            &ProjectName::new("ra").unwrap(),
            &TagName::new("v1.0.0").unwrap(),
        )
        .expect("read");
    assert_eq!(snap.header().project.as_str(), "ra");
}

#[test]
fn corrupted_snapshot_surfaces_a_store_read_error() {
    let dir = tempdir().unwrap();
    write_canonical(dir.path(), "ra", "v1.0.0");
    let path = dir.path().join("ra").join("v1.0.0.api.txt");
    fs::write(&path, "this is not a valid snapshot").unwrap();
    let store = SnapshotStore::open(dir.path()).unwrap();
    let r = store.read(
        &ProjectName::new("ra").unwrap(),
        &TagName::new("v1.0.0").unwrap(),
    );
    assert!(r.is_err());
}
