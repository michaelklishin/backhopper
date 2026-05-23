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
use backhopper_cli::commands::auto_generate::{missing_pins, missing_snapshots_error};

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

#[test]
fn missing_snapshots_error_picks_version_oldest_not_lex_min() {
    // Lex min of ["v2.10.0", "v2.9.0"] is "v2.10.0" (because '1' < '9'),
    // but the version-oldest is "v2.9.0". A lex-based --since would
    // exclude v2.9.0 entirely.
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
