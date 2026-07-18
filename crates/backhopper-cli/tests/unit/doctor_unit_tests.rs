// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Pure-function tests for `doctor` helpers that do not need a git
//! repository. End-to-end behavior is covered by
//! `tests/integration/end_to_end_doctor_integration_tests.rs`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use tempfile::TempDir;
use time::OffsetDateTime;

use backhopper_core::config::Config;
use backhopper_core::model::names::{CommitSha, GitRef, ProjectName, TagGlob, TagName};
use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader, state};
use backhopper_core::store::SnapshotStore;

use backhopper_cli::CommandOutcome;
use backhopper_cli::commands::doctor::{
    SnapshotStatus, Totals, build_pin_row, count_newer_tags, doctor_exit_code, snapshot_cell,
    staleness_note,
};

fn tag(s: &str) -> TagName {
    TagName::new(s).unwrap()
}

fn pin(project: &str, tag_name: &str) -> Pin {
    Pin::new(ProjectName::new(project).unwrap(), tag(tag_name))
}

fn project(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

fn write_snapshot_with_version(store_root: &Path, project: &str, tag_name: &str, version: &str) {
    fs::create_dir_all(store_root).unwrap();
    let store = SnapshotStore::open_mut(store_root).unwrap();
    let header = SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag_name).unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: version.to_owned(),
        dep_pins: Vec::new(),
    };
    let snap = Snapshot::<state::Unsorted>::from_extracted(header, Vec::new(), Vec::new())
        .into_canonical();
    store.write(&snap).unwrap();
}

fn load_cfg(dir: &Path) -> Config {
    let body = r#"
config_version = 1
[defaults]
snapshot_dir = "snapshots"
[[project]]
name    = "gen_batch_server"
git_url = "/nonexistent/gen_batch_server.git"
"#;
    let path = dir.join("backhopper.toml");
    fs::write(&path, body).unwrap();
    Config::load(&path).unwrap()
}

#[test]
fn no_resolved_counts_every_tag_as_ahead() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0"), tag("v3.0.0")];
    assert_eq!(count_newer_tags(&tags, None), 3);
}

#[test]
fn resolved_tag_counts_only_strictly_newer_tags() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0"), tag("v3.0.0")];
    let resolved = tag("v2.0.0");
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 1);
}

#[test]
fn semver_ordering_is_numeric_not_lexicographic() {
    let tags = vec![tag("v2.9.0"), tag("v2.10.0"), tag("v2.11.0"), tag("v3.0.0")];
    let resolved = tag("v2.10.0");
    // numerically newer than v2.10.0: v2.11.0 and v3.0.0; lexicographic order would misplace v2.9.0
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 2);
}

#[test]
fn resolved_equal_to_newest_yields_zero_ahead() {
    let tags = vec![tag("v1.0.0"), tag("v2.0.0")];
    let resolved = tag("v2.0.0");
    assert_eq!(count_newer_tags(&tags, Some(&resolved)), 0);
}

#[test]
fn empty_filtered_set_yields_zero() {
    let resolved = tag("v1.0.0");
    assert_eq!(count_newer_tags(&[], Some(&resolved)), 0);
    assert_eq!(count_newer_tags(&[], None), 0);
}

#[test]
fn literal_pin_behind_the_store_suggests_series_sync_diff() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.16.0"));
    let resolved = pin("cowlib", "2.16.0");
    let newest = tag("2.17.1");
    let note = staleness_note(&spec, Some(&resolved), Some(&newest)).unwrap();
    assert!(note.contains("2.17.1"), "{note}");
    assert!(note.contains("series sync diff"), "{note}");
}

#[test]
fn pattern_pin_excluded_by_glob_names_the_newer_tag() {
    let spec = PinSpec::Pattern {
        project: ProjectName::new("cowlib").unwrap(),
        pattern: TagGlob::new("2.16.*").unwrap(),
        select: PinSelect::Latest,
    };
    let resolved = pin("cowlib", "2.16.0");
    let newest = tag("2.17.1");
    let note = staleness_note(&spec, Some(&resolved), Some(&newest)).unwrap();
    assert!(note.contains("pattern excludes"), "{note}");
    assert!(note.contains("2.17.1"), "{note}");
}

#[test]
fn pin_at_the_newest_tag_gets_no_staleness_note() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.17.1"));
    let resolved = pin("cowlib", "2.17.1");
    let newest = tag("2.17.1");
    assert_eq!(staleness_note(&spec, Some(&resolved), Some(&newest)), None);
}

#[test]
fn self_pin_gets_no_staleness_note() {
    let spec = PinSpec::SelfRef {
        project: ProjectName::new("rabbit").unwrap(),
        git_ref: GitRef::new("v4.1.x").unwrap(),
        repo_dir_path: None,
    };
    let resolved = pin("rabbit", "v4.1.0");
    let newest = tag("v4.2.0");
    assert_eq!(staleness_note(&spec, Some(&resolved), Some(&newest)), None);
}

#[test]
fn unresolved_pin_gets_no_staleness_note() {
    let spec = PinSpec::literal(ProjectName::new("cowlib").unwrap(), tag("2.16.0"));
    let newest = tag("2.17.1");
    assert_eq!(staleness_note(&spec, None, Some(&newest)), None);
}

#[test]
fn stale_extractor_pin_is_flagged_with_remedy_and_nonzero_exit() {
    let tmp = TempDir::new().unwrap();
    let store_root = tmp.path().join("snapshots");
    write_snapshot_with_version(&store_root, "gen_batch_server", "v0.8.8", "1");
    let cfg = load_cfg(tmp.path());
    let store = SnapshotStore::open(&store_root).unwrap();
    let spec = PinSpec::literal(project("gen_batch_server"), tag("v0.8.8"));
    let row = build_pin_row(&cfg, &store, &spec, false, &mut BTreeMap::new()).unwrap();
    assert!(
        matches!(row.snapshot(), SnapshotStatus::Stale { stored, .. } if stored == "1"),
        "snapshot status: {:?}",
        row.snapshot()
    );
    assert_eq!(snapshot_cell(row.snapshot()), "STALE");
    let note = row.note().expect("stale pin carries a note");
    assert!(note.contains("extractor stored="), "{note}");
    assert!(note.contains("snapshots generate"), "{note}");
    let mut totals = Totals::default();
    totals.record(row.snapshot());
    assert_eq!(totals.stale_extractor, 1);
    assert_ne!(doctor_exit_code(&totals), CommandOutcome::Success);
}

#[test]
fn present_current_pin_reports_ok_and_zero_exit() {
    let tmp = TempDir::new().unwrap();
    let store_root = tmp.path().join("snapshots");
    write_snapshot_with_version(
        &store_root,
        "gen_batch_server",
        "v0.8.8",
        backhopper_erlang::EXTRACTOR_VERSION,
    );
    let cfg = load_cfg(tmp.path());
    let store = SnapshotStore::open(&store_root).unwrap();
    let spec = PinSpec::literal(project("gen_batch_server"), tag("v0.8.8"));
    let row = build_pin_row(&cfg, &store, &spec, false, &mut BTreeMap::new()).unwrap();
    assert_eq!(row.snapshot(), &SnapshotStatus::Current);
    assert_eq!(snapshot_cell(row.snapshot()), "ok");
    let mut totals = Totals::default();
    totals.record(row.snapshot());
    assert_eq!(totals.present, 1);
    assert_eq!(totals.stale_extractor, 0);
    assert_eq!(doctor_exit_code(&totals), CommandOutcome::Success);
}

#[test]
fn missing_alongside_stale_counts_both_and_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    let store_root = tmp.path().join("snapshots");
    write_snapshot_with_version(&store_root, "gen_batch_server", "v0.8.8", "1");
    let cfg = load_cfg(tmp.path());
    let store = SnapshotStore::open(&store_root).unwrap();
    let stale_spec = PinSpec::literal(project("gen_batch_server"), tag("v0.8.8"));
    let missing_spec = PinSpec::literal(project("gen_batch_server"), tag("v0.9.0"));
    let mut store_tags = BTreeMap::new();
    let stale_row = build_pin_row(&cfg, &store, &stale_spec, false, &mut store_tags).unwrap();
    let missing_row = build_pin_row(&cfg, &store, &missing_spec, false, &mut store_tags).unwrap();
    assert_eq!(snapshot_cell(missing_row.snapshot()), "MISSING");
    let mut totals = Totals::default();
    totals.record(stale_row.snapshot());
    totals.record(missing_row.snapshot());
    assert_eq!(totals.stale_extractor, 1);
    assert_eq!(totals.missing, 1);
    assert_eq!(totals.present, 1);
    assert_ne!(doctor_exit_code(&totals), CommandOutcome::Success);
}

#[test]
fn doctor_exit_code_is_zero_when_clean() {
    assert_eq!(
        doctor_exit_code(&Totals::default()),
        CommandOutcome::Success
    );
}

#[test]
fn doctor_exit_code_is_nonzero_for_missing() {
    let totals = Totals {
        missing: 1,
        ..Totals::default()
    };
    assert_ne!(doctor_exit_code(&totals), CommandOutcome::Success);
}

#[test]
fn doctor_exit_code_is_nonzero_for_stale_extractor() {
    let totals = Totals {
        stale_extractor: 1,
        ..Totals::default()
    };
    assert_ne!(doctor_exit_code(&totals), CommandOutcome::Success);
}
