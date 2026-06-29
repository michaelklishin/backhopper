// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Coverage classification shared by `doctor` and `snapshots verify`.

use std::str::FromStr;

use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_cli::commands::pin_coverage::{PinCoverage, classify_pin};
use backhopper_core::model::names::{CommitSha, GitRef, ProjectName, TagGlob, TagName};
use backhopper_core::model::pin::{PinSelect, PinSpec};
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
        dep_pins: Vec::new(),
    }
}

fn write_snapshot(root: &std::path::Path, project: &str, tag: &str) {
    let store = SnapshotStore::open_mut(root).unwrap();
    let snap =
        Snapshot::<state::Unsorted>::from_extracted(header(project, tag), Vec::new(), Vec::new())
            .into_canonical();
    store.write(&snap).unwrap();
}

fn project(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

#[test]
fn literal_pin_with_snapshot_on_disk_is_covered() {
    let tmp = TempDir::new().unwrap();
    write_snapshot(tmp.path(), "ra", "v2.16.11");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let spec = PinSpec::literal(project("ra"), TagName::new("v2.16.11").unwrap());
    match classify_pin(&spec, &store) {
        PinCoverage::Resolved { pin, present } => {
            assert!(present);
            assert_eq!(pin.project, project("ra"));
            assert_eq!(pin.tag, TagName::new("v2.16.11").unwrap());
        }
        other => panic!("expected covered, got {other:?}"),
    }
}

#[test]
fn literal_pin_without_snapshot_is_missing() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let spec = PinSpec::literal(project("khepri"), TagName::new("v0.16.0").unwrap());
    assert!(matches!(
        classify_pin(&spec, &store),
        PinCoverage::Resolved { present: false, .. }
    ));
}

#[test]
fn self_ref_pin_short_circuits_to_self_pin() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let spec = PinSpec::SelfRef {
        project: project("rabbit"),
        git_ref: GitRef::new("v4.1.x").unwrap(),
        repo_dir_path: None,
    };
    assert_eq!(classify_pin(&spec, &store), PinCoverage::SelfPin);
}

#[test]
fn pattern_pin_matching_no_stored_tag_is_unresolved() {
    let tmp = TempDir::new().unwrap();
    write_snapshot(tmp.path(), "osiris", "v1.8.6");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let spec = PinSpec::pattern(
        project("osiris"),
        TagGlob::new("v2.*").unwrap(),
        PinSelect::Latest,
    );
    assert_eq!(
        classify_pin(&spec, &store),
        PinCoverage::Unresolved {
            project: project("osiris")
        }
    );
}

#[test]
fn pattern_pin_resolving_to_present_tag_is_covered() {
    let tmp = TempDir::new().unwrap();
    write_snapshot(tmp.path(), "osiris", "v1.8.6");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let spec = PinSpec::pattern(
        project("osiris"),
        TagGlob::new("v1.*").unwrap(),
        PinSelect::Latest,
    );
    match classify_pin(&spec, &store) {
        PinCoverage::Resolved { pin, present } => {
            assert!(present);
            assert_eq!(pin.tag, TagName::new("v1.8.6").unwrap());
        }
        other => panic!("expected covered, got {other:?}"),
    }
}
