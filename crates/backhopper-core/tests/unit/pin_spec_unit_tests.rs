// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use backhopper_core::errors::ConfigError;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagGlob, TagName};
use backhopper_core::model::pin::{self, PinSelect, PinSelector, PinSpec};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader};
use backhopper_core::store::{ReadOnly, SnapshotStore};
use tempfile::TempDir;

fn store_with_tags(project: &ProjectName, tags: &[&str]) -> (TempDir, SnapshotStore<ReadOnly>) {
    let tmp = TempDir::new().unwrap();
    let mut_store = SnapshotStore::open_mut(tmp.path()).unwrap();
    for tag_str in tags {
        let tag = TagName::new(*tag_str).unwrap();
        let header = SnapshotHeader {
            project: project.clone(),
            tag: tag.clone(),
            branch: None,
            commit: CommitSha::new("0".repeat(40)).unwrap(),
            scanned_paths: Vec::new(),
            apps_scanned: Vec::new(),
            generated_by: "backhopper 0.0.0".into(),
            generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
            extractor_version: String::new(),
            dep_pins: Vec::new(),
        };
        let snap = Snapshot::from_extracted(header, Vec::new(), Vec::new()).into_canonical();
        mut_store.write(&snap).unwrap();
    }
    let read_store = SnapshotStore::open(tmp.path().to_path_buf()).unwrap();
    (tmp, read_store)
}

fn project() -> ProjectName {
    ProjectName::new("otp").unwrap()
}

#[test]
fn literal_spec_passes_through_unchanged() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &[]);
    let spec = PinSpec::literal(p.clone(), TagName::new("v1.2.3").unwrap());
    let pin = spec.resolve(&store).unwrap();
    assert_eq!(pin.project, p);
    assert_eq!(pin.tag.as_str(), "v1.2.3");
}

#[test]
fn pattern_latest_picks_highest_version_match() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &["OTP-26.0", "OTP-26.2.5", "OTP-26.2.6", "OTP-27.0"]);
    let spec = PinSpec::pattern(
        p.clone(),
        TagGlob::new("OTP-26.*").unwrap(),
        PinSelect::Latest,
    );
    let pin = spec.resolve(&store).unwrap();
    assert_eq!(pin.tag.as_str(), "OTP-26.2.6");
}

#[test]
fn pattern_oldest_picks_lowest_version_match() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &["OTP-26.0", "OTP-26.2.5", "OTP-26.2.6"]);
    let spec = PinSpec::pattern(
        p.clone(),
        TagGlob::new("OTP-26.*").unwrap(),
        PinSelect::Oldest,
    );
    let pin = spec.resolve(&store).unwrap();
    assert_eq!(pin.tag.as_str(), "OTP-26.0");
}

#[test]
fn pattern_with_no_matches_returns_hard_error() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &["OTP-26.0", "OTP-27.0"]);
    let spec = PinSpec::pattern(
        p.clone(),
        TagGlob::new("OTP-28.*").unwrap(),
        PinSelect::Latest,
    );
    match spec.resolve(&store) {
        Err(ConfigError::PinPatternNoMatch { project, pattern }) => {
            assert_eq!(project, "otp");
            assert_eq!(pattern, "OTP-28.*");
        }
        other => panic!("expected PinPatternNoMatch, got {other:?}"),
    }
}

#[test]
fn pattern_matches_only_tags_for_this_project() {
    let p = project();
    let other = ProjectName::new("ra").unwrap();
    let tmp = TempDir::new().unwrap();
    let mut_store = SnapshotStore::open_mut(tmp.path()).unwrap();
    for (proj, tag_str) in [(&p, "OTP-26.0"), (&other, "OTP-26.99")] {
        let tag = TagName::new(tag_str).unwrap();
        let header = SnapshotHeader {
            project: proj.clone(),
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
        mut_store.write(&snap).unwrap();
    }
    let store = SnapshotStore::open(tmp.path().to_path_buf()).unwrap();
    let spec = PinSpec::pattern(
        p.clone(),
        TagGlob::new("OTP-26.*").unwrap(),
        PinSelect::Latest,
    );
    let pin = spec.resolve(&store).unwrap();
    assert_eq!(pin.tag.as_str(), "OTP-26.0");
}

#[test]
fn project_method_returns_project_for_both_variants() {
    let p = project();
    let lit = PinSpec::literal(p.clone(), TagName::new("v1.0").unwrap());
    let pat = PinSpec::pattern(p.clone(), TagGlob::new("v1.*").unwrap(), PinSelect::Latest);
    assert_eq!(lit.project(), &p);
    assert_eq!(pat.project(), &p);
}

#[test]
fn pin_select_pick_returns_none_for_empty_input() {
    let empty: Vec<&TagName> = Vec::new();
    assert!(PinSelect::Latest.pick(empty.iter().copied()).is_none());
    assert!(PinSelect::Oldest.pick(empty.iter().copied()).is_none());
}

#[test]
fn resolve_all_returns_one_pin_per_spec_in_order() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &["OTP-26.2.5", "OTP-27.0"]);
    let specs = vec![
        PinSpec::pattern(
            p.clone(),
            TagGlob::new("OTP-26.*").unwrap(),
            PinSelect::Latest,
        ),
        PinSpec::pattern(
            p.clone(),
            TagGlob::new("OTP-27.*").unwrap(),
            PinSelect::Latest,
        ),
        PinSpec::literal(p.clone(), TagName::new("OTP-26.2.5").unwrap()),
    ];
    let pins = pin::resolve_all(&specs, &store).unwrap();
    assert_eq!(pins.len(), 3);
    assert_eq!(pins[0].tag.as_str(), "OTP-26.2.5");
    assert_eq!(pins[1].tag.as_str(), "OTP-27.0");
    assert_eq!(pins[2].tag.as_str(), "OTP-26.2.5");
}

#[test]
fn as_self_pin_is_none_for_literal_and_pattern() {
    let p = project();
    let lit = PinSpec::literal(p.clone(), TagName::new("v1.0").unwrap());
    let pat = PinSpec::pattern(p.clone(), TagGlob::new("v1.*").unwrap(), PinSelect::Latest);
    assert!(lit.as_self_pin().is_none());
    assert!(pat.as_self_pin().is_none());
}

#[test]
fn as_self_pin_carries_the_override_path() {
    use backhopper_core::model::names::GitRef;

    let p = project();
    let with_override = PinSpec::SelfRef {
        project: p.clone(),
        git_ref: GitRef::new("main").unwrap(),
        repo_dir_path: Some(PathBuf::from("/srv/host.git")),
    };
    let self_pin = with_override.as_self_pin().unwrap();
    assert_eq!(self_pin.project, &p);
    assert_eq!(self_pin.repo_dir_path, Some(Path::new("/srv/host.git")));

    let without_override = PinSpec::SelfRef {
        project: p.clone(),
        git_ref: GitRef::new("main").unwrap(),
        repo_dir_path: None,
    };
    assert!(
        without_override
            .as_self_pin()
            .unwrap()
            .repo_dir_path
            .is_none()
    );
}

#[test]
fn resolve_all_short_circuits_on_first_failure() {
    let p = project();
    let (_tmp, store) = store_with_tags(&p, &["OTP-26.0"]);
    let specs = vec![
        PinSpec::pattern(
            p.clone(),
            TagGlob::new("OTP-26.*").unwrap(),
            PinSelect::Latest,
        ),
        PinSpec::pattern(
            p.clone(),
            TagGlob::new("OTP-28.*").unwrap(),
            PinSelect::Latest,
        ),
    ];
    assert!(pin::resolve_all(&specs, &store).is_err());
}

#[test]
fn series_name_is_the_series_arm_alone() {
    let series = SeriesName::new("v4.1.x").unwrap();
    assert_eq!(
        PinSelector::Series(series.clone()).series_name(),
        Some(&series)
    );
    let pin = PinSelector::pin(project(), TagName::new("v2.16.0").unwrap());
    assert_eq!(pin.series_name(), None);
}
