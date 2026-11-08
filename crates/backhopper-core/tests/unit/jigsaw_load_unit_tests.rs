// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::convert::Infallible;
use std::fs;

use tempfile::TempDir;

use backhopper_core::jigsaw::plugin_set::{PluginSet, PluginSources};
use backhopper_core::jigsaw::references::ReferenceSet;
use backhopper_core::jigsaw::{
    JigsawPlan, ListedTags, PlanError, ResolvedRelease, SeriesRequest, SnapshotGap, StaleReason,
    StoreStatus, TagSourceKind, UnstockedPlan, evaluate, plan_series,
};
use backhopper_core::model::names::{ExtractionSettingsDigest, HostFactsDigest};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::ErlangMajor;
use backhopper_core::model::snapshot::HostReleaseFacts;
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};

use crate::jigsaw_support::{
    StoredSnapshot, commit, config, expected, extractor, hex_pin, project, rabbitmq, tag, tags,
};

fn plan() -> JigsawPlan {
    let cfg = config();
    plan_series(
        &cfg,
        rabbitmq(&cfg),
        &SeriesRequest {
            series: "4.2".parse().unwrap(),
            up_to: None,
            majors: NonEmpty::new(ErlangMajor::try_from(27).unwrap(), []),
        },
        &ListedTags {
            host: tags(&["v4.2.0", "v4.2.1"]),
            otp: tags(&["OTP-27.3", "OTP-27.3.1"]),
        },
        TagSourceKind::Repository,
        extractor(),
        |release| {
            let (digit, ra) = if release.tag().as_str() == "v4.2.0" {
                ('1', "2.17.1")
            } else {
                ('2', "2.17.3")
            };
            Ok::<_, PlanError>(ResolvedRelease {
                commit: commit(digit),
                pins: vec![hex_pin("ra", ra)],
            })
        },
    )
    .unwrap()
    .plan
}

fn stock(store: &SnapshotStore<Mutable>) {
    let cfg = config();
    StoredSnapshot::host(&cfg, "v4.2.0", commit('1'), vec![hex_pin("ra", "2.17.1")]).write(store);
    StoredSnapshot::host(&cfg, "v4.2.1", commit('2'), vec![hex_pin("ra", "2.17.3")]).write(store);
    StoredSnapshot::dependency("ra", "v2.17.1", "ra").write(store);
    StoredSnapshot::dependency("ra", "v2.17.3", "ra").write(store);
    StoredSnapshot::otp("OTP-27.3").write(store);
    StoredSnapshot::otp("OTP-27.3.1").write(store);
}

fn fixture() -> (TempDir, SnapshotStore<Mutable>) {
    let dir = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    stock(&store);
    (dir, store)
}

fn read_only(dir: &TempDir) -> SnapshotStore<ReadOnly> {
    SnapshotStore::open(dir.path()).unwrap()
}

fn unstocked(plan: &JigsawPlan, store: &SnapshotStore<ReadOnly>) -> UnstockedPlan {
    let StoreStatus::Unstocked(status) = plan.store_status(store, expected) else {
        panic!("the store should not be stocked");
    };
    let loaded = plan.clone().load(store, expected).unwrap_err();
    assert_eq!(
        status.gaps(),
        loaded.gaps(),
        "store_status and load disagree"
    );
    loaded
}

#[test]
fn a_stocked_store_loads_every_required_snapshot() {
    let (dir, _store) = fixture();
    let store = read_only(&dir);
    let plan = plan();
    assert_eq!(plan.store_status(&store, expected), StoreStatus::Stocked);
    let loaded = plan.clone().load(&store, expected).unwrap();
    assert_eq!(loaded.snapshots().len(), plan.required().len());
    assert_eq!(loaded.plan(), &plan);
}

#[test]
fn the_apps_of_the_build_patch_surface_are_collected() {
    let (dir, _store) = fixture();
    let loaded = plan().load(&read_only(&dir), expected).unwrap();
    let names: Vec<&str> = loaded
        .build_app_names()
        .iter()
        .map(|app| app.as_str())
        .collect();
    assert_eq!(names, ["kernel", "ra", "rabbit"]);
}

#[test]
fn the_store_digest_is_stable_and_follows_what_was_read() {
    let (dir, store) = fixture();
    let plan = plan();
    let first = plan.clone().load(&read_only(&dir), expected).unwrap();
    let again = plan.clone().load(&read_only(&dir), expected).unwrap();
    assert_eq!(first.store_digest(), again.store_digest());
    let mut moved = StoredSnapshot::dependency("ra", "v2.17.3", "ra");
    moved.commit = commit('e');
    moved.write(&store);
    let after = plan.load(&read_only(&dir), expected).unwrap();
    assert_ne!(first.store_digest(), after.store_digest());
}

#[test]
fn a_missing_snapshot_is_reported_by_pin() {
    let (dir, store) = fixture();
    store.delete(&project("ra"), &tag("v2.17.1")).unwrap();
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.missing.len(), 1);
    assert_eq!(status.missing[0].to_string(), "ra@v2.17.1");
    assert!(status.stale.is_empty() && status.commit_mismatch.is_empty());
}

#[test]
fn a_host_snapshot_at_another_commit_is_a_commit_mismatch() {
    let (dir, store) = fixture();
    let cfg = config();
    StoredSnapshot::host(&cfg, "v4.2.0", commit('9'), vec![hex_pin("ra", "2.17.1")]).write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.commit_mismatch.len(), 1);
    assert_eq!(status.commit_mismatch[0].expected, commit('1'));
    assert_eq!(status.commit_mismatch[0].found, commit('9'));
    assert_eq!(status.gaps()[0].1, SnapshotGap::CommitMismatch);
}

#[test]
fn a_snapshot_from_an_older_extractor_is_stale() {
    let (dir, store) = fixture();
    let mut old = StoredSnapshot::dependency("ra", "v2.17.1", "ra");
    old.extractor = "8";
    old.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(
        status.stale[0].reason,
        StaleReason::ExtractorVersion {
            found: "8".to_owned()
        }
    );
}

#[test]
fn a_snapshot_below_the_current_format_is_stale() {
    let (dir, store) = fixture();
    let mut old = StoredSnapshot::dependency("ra", "v2.17.1", "ra");
    old.format_version = 4;
    old.settings = None;
    old.apps.clear();
    old.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(
        status.stale[0].reason,
        StaleReason::FormatVersion { found: 4 }
    );
}

#[test]
fn a_host_snapshot_with_other_host_facts_is_stale() {
    let (dir, store) = fixture();
    let cfg = config();
    let mut drifted =
        StoredSnapshot::host(&cfg, "v4.2.0", commit('1'), vec![hex_pin("ra", "2.17.1")]);
    let facts = drifted.host_facts.as_ref().unwrap();
    drifted.host_facts = Some(HostReleaseFacts {
        release_roots: facts.release_roots.clone(),
        digest: HostFactsDigest::new("0".repeat(64)).unwrap(),
    });
    drifted.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.stale[0].reason, StaleReason::HostFacts);
}

#[test]
fn a_snapshot_made_with_other_extraction_settings_is_stale_with_the_reason_in_its_detail() {
    let (dir, store) = fixture();
    let mut other = StoredSnapshot::dependency("ra", "v2.17.1", "ra");
    other.settings = Some(ExtractionSettingsDigest::new("0".repeat(64)).unwrap());
    other.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.stale[0].reason, StaleReason::ExtractionSettings);
    assert_eq!(
        status.detail(&status.stale[0].pin).as_deref(),
        Some("made with other extraction settings than the configured ones")
    );
}

#[test]
fn a_snapshot_that_records_no_extraction_settings_is_stale() {
    let (dir, store) = fixture();
    let mut bare = StoredSnapshot::dependency("ra", "v2.17.1", "ra");
    bare.settings = None;
    bare.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.stale[0].reason, StaleReason::ExtractionSettings);
}

#[test]
fn a_pin_of_a_project_that_is_not_configured_is_missing() {
    let (dir, _store) = fixture();
    let StoreStatus::Unstocked(status) = plan().store_status(&read_only(&dir), |project| {
        (project.as_str() != "ra")
            .then(|| expected(project))
            .flatten()
    }) else {
        panic!("a project without an expectation cannot be stocked");
    };
    assert!(
        status
            .missing
            .iter()
            .all(|pin| pin.project.as_str() == "ra")
    );
    assert_eq!(status.missing.len(), 2);
}

#[test]
fn a_host_snapshot_without_host_facts_is_stale() {
    let (dir, store) = fixture();
    let cfg = config();
    let mut bare = StoredSnapshot::host(&cfg, "v4.2.1", commit('2'), vec![hex_pin("ra", "2.17.3")]);
    bare.host_facts = None;
    bare.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.stale[0].reason, StaleReason::HostFacts);
}

#[test]
fn a_file_that_does_not_parse_is_stale() {
    let (dir, store) = fixture();
    let path = store
        .snapshot_path(&project("otp"), &tag("OTP-27.3"))
        .unwrap();
    fs::write(path, "not a snapshot\n").unwrap();
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.stale[0].reason, StaleReason::Unreadable);
}

#[test]
fn every_gap_is_reported_at_once() {
    let (dir, store) = fixture();
    store.delete(&project("ra"), &tag("v2.17.3")).unwrap();
    let mut old = StoredSnapshot::otp("OTP-27.3.1");
    old.extractor = "8";
    old.write(&store);
    let status = unstocked(&plan(), &read_only(&dir));
    assert_eq!(status.gaps().len(), 2);
}

#[test]
fn a_run_with_no_store_has_everything_missing() {
    let plan = plan();
    let StoreStatus::Unstocked(status) = plan.nothing_stored() else {
        panic!("nothing is stored");
    };
    assert_eq!(status.missing.len(), plan.required().len());
}

#[test]
fn evaluation_takes_the_loaded_plan() {
    let (dir, _store) = fixture();
    let loaded = plan().load(&read_only(&dir), expected).unwrap();
    let plugins = PluginSet::new(PluginSources::default()).unwrap();
    let report = evaluate(&loaded, &plugins, |_, _| {
        Ok::<_, Infallible>(ReferenceSet::new())
    })
    .unwrap();
    assert_eq!(report.build_release().tag().as_str(), "v4.2.1");
    assert_eq!(report.targets().hosts().len(), 2);
    assert_eq!(report.targets().otp_tags().len(), 2);
    assert!(report.is_clean());
}
