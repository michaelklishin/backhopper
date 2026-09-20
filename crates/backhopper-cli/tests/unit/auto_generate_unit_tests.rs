// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Tests for `auto_generate` helpers: pin-presence detection and the
//! structured `MissingSnapshots` error built from missing pins.

use tempfile::TempDir;

use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::store::SnapshotStore;

use bel7_cli::{ExitCode, ExitCodeProvider};

use backhopper_cli::CliError;
use backhopper_cli::commands::auto_generate::{
    ExtractorFreshness, GenerateAction, PinCoverageStatus, coverage_report, extractor_freshness,
    generate_action, missing_pins, missing_snapshots_error,
};
use backhopper_core::config::Config;
use backhopper_core::model::names::CommitSha;
use backhopper_core::model::snapshot::{FORMAT_VERSION, Snapshot, SnapshotHeader, state};
use std::str::FromStr;
use time::OffsetDateTime;

fn pin(project: &str, tag: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new(tag).unwrap(),
    )
}

#[test]
fn missing_pins_reports_every_absent_pin() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let pins = vec![pin("ra", "v2.16.13"), pin("khepri", "v0.17.0")];
    let missing = missing_pins(&store, &pins);
    assert_eq!(missing, pins);
}

#[test]
fn missing_snapshots_error_lists_pins_in_detail() {
    let pins = vec![pin("ra", "v2.16.13"), pin("khepri", "v0.17.0")];
    let err = missing_snapshots_error(&pins);
    let detail = err
        .detail()
        .expect("MissingSnapshots has structured detail");
    assert!(detail.contains("ra @ v2.16.13"));
    assert!(detail.contains("khepri @ v0.17.0"));
}

#[test]
fn missing_snapshots_error_groups_remediation_by_project() {
    let pins = vec![
        pin("ra", "v2.16.13"),
        pin("ra", "v2.17.0"),
        pin("khepri", "v0.17.0"),
    ];
    let err = missing_snapshots_error(&pins);
    let hint = err.hint().expect("MissingSnapshots has a hint");
    assert!(hint.contains("--project ra"));
    assert!(hint.contains("--project khepri"));
    // The remediation should pick the oldest tag for ra so --since includes both.
    assert!(hint.contains("v2.16.13"));
}

fn synthetic_header(project: &str, tag: &str, extractor_version: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: extractor_version.to_owned(),
        format_version: FORMAT_VERSION,
        dep_pins: Vec::new(),
    }
}

fn write_synthetic_snapshot(
    store_path: &std::path::Path,
    project: &str,
    tag: &str,
    extractor_version: &str,
) {
    let mut_store = SnapshotStore::open_mut(store_path).unwrap();
    let snap = Snapshot::<state::Unsorted>::from_extracted(
        synthetic_header(project, tag, extractor_version),
        Vec::new(),
        Vec::new(),
    )
    .into_canonical();
    mut_store.write(&snap).unwrap();
}

fn empty_config(dir: &std::path::Path) -> Config {
    let cfg_path = dir.join("backhopper.toml");
    std::fs::write(&cfg_path, "").unwrap();
    Config::load(&cfg_path).expect("trivial config parses")
}

#[test]
fn coverage_report_marks_missing_pins() {
    let tmp = TempDir::new().unwrap();
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let pins = vec![pin("ra", "v3.1.7")];
    let report = coverage_report(&empty_config(tmp.path()), &store, &pins);
    assert_eq!(report.len(), 1);
    assert!(matches!(report[0].status, PinCoverageStatus::Missing));
}

#[test]
fn coverage_report_marks_present_pins() {
    let tmp = TempDir::new().unwrap();
    let extractor = backhopper_erlang::EXTRACTOR_VERSION;
    write_synthetic_snapshot(tmp.path(), "ra", "v3.1.7", extractor);
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let pins = vec![pin("ra", "v3.1.7")];
    let report = coverage_report(&empty_config(tmp.path()), &store, &pins);
    assert_eq!(report.len(), 1);
    assert!(matches!(report[0].status, PinCoverageStatus::Present));
}

#[test]
fn coverage_report_flags_stale_extractor() {
    let tmp = TempDir::new().unwrap();
    write_synthetic_snapshot(tmp.path(), "ra", "v3.1.7", "stale-version");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let pins = vec![pin("ra", "v3.1.7")];
    let report = coverage_report(&empty_config(tmp.path()), &store, &pins);
    assert_eq!(report.len(), 1);
    match &report[0].status {
        PinCoverageStatus::StaleExtractor { stored, expected } => {
            assert_eq!(stored, "stale-version");
            assert_eq!(expected, backhopper_erlang::EXTRACTOR_VERSION);
        }
        other => panic!("expected StaleExtractor, got {other:?}"),
    }
}

#[test]
fn coverage_report_flags_an_empty_extractor_version_as_unversioned() {
    // a snapshot with no extractor-version header predates versioning
    // and is the most suspect file in the store, not the healthiest
    let tmp = TempDir::new().unwrap();
    write_synthetic_snapshot(tmp.path(), "ra", "v3.1.7", "");
    let store = SnapshotStore::open(tmp.path()).unwrap();
    let pins = vec![pin("ra", "v3.1.7")];
    let report = coverage_report(&empty_config(tmp.path()), &store, &pins);
    assert_eq!(report.len(), 1);
    match &report[0].status {
        PinCoverageStatus::Unversioned {
            format_version,
            expected,
        } => {
            assert_eq!(*format_version, FORMAT_VERSION);
            assert_eq!(expected, backhopper_erlang::EXTRACTOR_VERSION);
        }
        other => panic!("expected Unversioned, got {other:?}"),
    }
}

#[test]
fn missing_snapshots_error_picks_version_oldest_not_lex_min() {
    // lex min of ["v2.10.0", "v2.9.0"] is "v2.10.0": version-oldest is "v2.9.0"
    let pins = vec![pin("ra", "v2.10.0"), pin("ra", "v2.9.0")];
    let err = missing_snapshots_error(&pins);
    let hint = err.hint().unwrap();
    assert!(
        hint.contains("--since v2.9.0"),
        "hint should pick the version-oldest tag, got: {hint}"
    );
}

#[test]
fn missing_snapshots_error_carries_auto_generate_hint_when_multiple_projects() {
    let pins = vec![pin("ra", "v2.0.0"), pin("khepri", "v0.1.0")];
    let err = missing_snapshots_error(&pins);
    let hint = err.hint().unwrap();
    assert!(hint.contains("--auto-generate"));
}

#[test]
fn single_project_remediation_is_a_one_liner() {
    let pins = vec![pin("ra", "v2.16.13")];
    let err = missing_snapshots_error(&pins);
    let hint = err.hint().unwrap();
    assert!(hint.starts_with("run: backhopper snapshots generate"));
    assert!(!hint.contains("--auto-generate"));
}

#[test]
fn missing_snapshots_error_uses_no_input_exit_code() {
    let pins = vec![pin("ra", "v2.16.13")];
    let err = missing_snapshots_error(&pins);
    assert_eq!(
        <CliError as ExitCodeProvider>::exit_code(&err),
        ExitCode::NoInput
    );
}

#[test]
fn missing_snapshots_error_variant_round_trips() {
    let pins = vec![pin("ra", "v2.16.13")];
    let err = missing_snapshots_error(&pins);
    match err {
        CliError::MissingSnapshots { missing, .. } => {
            assert_eq!(missing.len(), 1);
            assert_eq!(missing[0].project.as_str(), "ra");
            assert_eq!(missing[0].tag.as_str(), "v2.16.13");
        }
        other => panic!("expected MissingSnapshots, got {other:?}"),
    }
}

fn header_with(extractor_version: &str, format_version: u32) -> SnapshotHeader {
    let mut header = synthetic_header("ra", "v3.1.7", extractor_version);
    header.format_version = format_version;
    header
}

#[test]
fn extractor_freshness_is_current_on_a_match() {
    let header = header_with("7", FORMAT_VERSION);
    assert_eq!(
        extractor_freshness(&header, "7"),
        ExtractorFreshness::Current
    );
}

#[test]
fn extractor_freshness_is_stale_on_a_mismatch() {
    let header = header_with("6", FORMAT_VERSION);
    assert_eq!(
        extractor_freshness(&header, "7"),
        ExtractorFreshness::Stale {
            stored: "6".to_owned()
        }
    );
}

#[test]
fn extractor_freshness_is_unversioned_for_an_empty_stored_version() {
    let header = header_with("", 1);
    assert_eq!(
        extractor_freshness(&header, "7"),
        ExtractorFreshness::Unversioned { format_version: 1 }
    );
}

#[test]
fn generate_action_builds_when_nothing_is_present() {
    assert_eq!(generate_action(None, false), GenerateAction::Build);
    assert_eq!(generate_action(None, true), GenerateAction::Build);
}

#[test]
fn generate_action_skips_a_current_snapshot_regardless_of_the_flag() {
    assert_eq!(
        generate_action(Some(&ExtractorFreshness::Current), false),
        GenerateAction::Skip
    );
    assert_eq!(
        generate_action(Some(&ExtractorFreshness::Current), true),
        GenerateAction::Skip
    );
}

#[test]
fn generate_action_skips_a_stale_snapshot_without_the_flag() {
    let stale = ExtractorFreshness::Stale {
        stored: "6".to_owned(),
    };
    assert_eq!(generate_action(Some(&stale), false), GenerateAction::Skip);
}

#[test]
fn generate_action_refreshes_a_stale_snapshot_with_the_flag() {
    let stale = ExtractorFreshness::Stale {
        stored: "6".to_owned(),
    };
    assert_eq!(generate_action(Some(&stale), true), GenerateAction::Refresh);
}

#[test]
fn generate_action_skips_an_unversioned_snapshot_without_the_flag() {
    let unversioned = ExtractorFreshness::Unversioned { format_version: 1 };
    assert_eq!(
        generate_action(Some(&unversioned), false),
        GenerateAction::Skip
    );
}

#[test]
fn generate_action_refreshes_an_unversioned_snapshot_with_the_flag() {
    let unversioned = ExtractorFreshness::Unversioned { format_version: 1 };
    assert_eq!(
        generate_action(Some(&unversioned), true),
        GenerateAction::Refresh
    );
}

#[test]
fn as_write_drops_skip_and_keeps_build_and_refresh() {
    assert_eq!(GenerateAction::Skip.as_write(), None);
    assert_eq!(
        GenerateAction::Build.as_write(),
        Some(backhopper_cli::commands::auto_generate::WriteKind::Build)
    );
    assert_eq!(
        GenerateAction::Refresh.as_write(),
        Some(backhopper_cli::commands::auto_generate::WriteKind::Refresh)
    );
}
