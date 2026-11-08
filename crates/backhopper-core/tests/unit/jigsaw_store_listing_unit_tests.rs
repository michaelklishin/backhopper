// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use tempfile::TempDir;

use backhopper_core::jigsaw::{PlanError, SeriesRequest, TagSourceKind, plan_series_from_store};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::{ErlangMajor, HostSeries};
use backhopper_core::store::{Mutable, SnapshotStore};

use crate::jigsaw_support::{
    StoredSnapshot, commit, config, extractor, hex_pin, project, rabbitmq, tag,
};

fn request(series: &str) -> SeriesRequest {
    SeriesRequest {
        series: series.parse::<HostSeries>().unwrap(),
        up_to: None,
        majors: NonEmpty::new(ErlangMajor::try_from(27).unwrap(), []),
    }
}

fn store_with_three_host_releases(dir: &TempDir) -> SnapshotStore<Mutable> {
    let cfg = config();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    for (release, digit, ra) in [
        ("v4.2.0", '1', "2.17.1"),
        ("v4.2.1", '2', "2.17.1"),
        ("v4.2.3", '3', "2.17.3"),
    ] {
        StoredSnapshot::host(
            &cfg,
            release,
            commit(digit),
            vec![hex_pin("ra", ra), hex_pin("jose", "1.11.10")],
        )
        .write(&store);
    }
    StoredSnapshot::otp("OTP-27.3").write(&store);
    store
}

#[test]
fn a_store_lists_only_the_releases_that_have_a_host_snapshot() {
    let dir = TempDir::new().unwrap();
    store_with_three_host_releases(&dir);
    let read_only = SnapshotStore::open(dir.path()).unwrap();
    let cfg = config();
    let drafted = plan_series_from_store(
        &cfg,
        rabbitmq(&cfg),
        &request("4.2"),
        &read_only,
        extractor(),
    )
    .unwrap();
    let plan = drafted.plan;
    assert_eq!(plan.tag_source(), TagSourceKind::Store);
    let tags: Vec<&str> = plan
        .releases()
        .iter()
        .map(|r| r.tag.tag().as_str())
        .collect();
    assert_eq!(tags, ["v4.2.0", "v4.2.1", "v4.2.3"]);
}

#[test]
fn commits_and_pins_come_from_the_snapshot_headers() {
    let dir = TempDir::new().unwrap();
    store_with_three_host_releases(&dir);
    let read_only = SnapshotStore::open(dir.path()).unwrap();
    let cfg = config();
    let plan = plan_series_from_store(
        &cfg,
        rabbitmq(&cfg),
        &request("4.2"),
        &read_only,
        extractor(),
    )
    .unwrap()
    .plan;
    let releases: Vec<_> = plan.releases().iter().collect();
    assert_eq!(releases[0].commit, commit('1'));
    assert_eq!(releases[2].commit, commit('3'));
    assert_eq!(releases[0].pins.get(&project("ra")), Some(&tag("v2.17.1")));
    assert_eq!(releases[2].pins.get(&project("ra")), Some(&tag("v2.17.3")));
    assert_eq!(plan.untracked_dependencies().len(), 1);
}

#[test]
fn a_series_without_stored_host_snapshots_is_empty() {
    let dir = TempDir::new().unwrap();
    store_with_three_host_releases(&dir);
    let read_only = SnapshotStore::open(dir.path()).unwrap();
    let cfg = config();
    let error = plan_series_from_store(
        &cfg,
        rabbitmq(&cfg),
        &request("4.3"),
        &read_only,
        extractor(),
    )
    .unwrap_err();
    assert!(matches!(error, PlanError::EmptySeries { .. }), "{error}");
}
