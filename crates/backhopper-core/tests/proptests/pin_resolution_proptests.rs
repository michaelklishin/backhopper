// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use time::OffsetDateTime;

use backhopper_core::model::names::{CommitSha, ProjectName, TagGlob, TagName};
use backhopper_core::model::pin::{PinSelect, PinSpec};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::store::SnapshotStore;
use proptest::prelude::*;
use tempfile::TempDir;

fn write_tag(
    store: &SnapshotStore<backhopper_core::store::Mutable>,
    project: &ProjectName,
    tag: &str,
) {
    let tag = TagName::new(tag.to_owned()).unwrap();
    let header = SnapshotHeader {
        project: project.clone(),
        tag: tag.clone(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "backhopper".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    };
    let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
    store.write(&snap).unwrap();
}

proptest! {
    #[test]
    fn pattern_resolution_is_invariant_under_tag_insertion_order(
        majors in proptest::collection::vec(0u32..30, 1..6),
    ) {
        let project = ProjectName::new("p").unwrap();

        // Build store with tags in original order
        let tmp_a = TempDir::new().unwrap();
        let store_a = SnapshotStore::open_mut(tmp_a.path()).unwrap();
        for m in &majors {
            write_tag(&store_a, &project, &format!("v{}.{}", m / 10, m % 10));
        }

        // Build store with tags in reversed order
        let tmp_b = TempDir::new().unwrap();
        let store_b = SnapshotStore::open_mut(tmp_b.path()).unwrap();
        for m in majors.iter().rev() {
            write_tag(&store_b, &project, &format!("v{}.{}", m / 10, m % 10));
        }

        let read_a = SnapshotStore::open(tmp_a.path().to_path_buf()).unwrap();
        let read_b = SnapshotStore::open(tmp_b.path().to_path_buf()).unwrap();

        let spec = PinSpec::pattern(
            project,
            TagGlob::new("v*").unwrap(),
            PinSelect::Latest,
        );
        prop_assert_eq!(
            spec.resolve(&read_a).unwrap().tag,
            spec.resolve(&read_b).unwrap().tag,
        );
    }
}
