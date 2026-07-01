// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Tri-state exit code helpers for the three commands that adopted
//! `bel7-cli` 0.12's `PARTIAL_SUCCESS_I32`: doctor, snapshots verify --all,
//! and xref backport-applicability.

use bel7_cli::PARTIAL_SUCCESS_I32;

use backhopper_cli::commands::doctor::{Totals, doctor_exit_code};
use backhopper_cli::commands::snapshots::verify_all_exit_code;
use backhopper_cli::commands::xref_backport_applicability::backport_applicability_exit_code;
use backhopper_core::model::verdict::exit;

fn totals(missing: usize, stale_extractor: usize) -> Totals {
    Totals {
        missing,
        stale_extractor,
        ..Totals::default()
    }
}

#[test]
fn verdict_module_constants_match_bel7_cli_convention() {
    assert_eq!(exit::OK, 0);
    assert_eq!(exit::NEEDS_ATTENTION, PARTIAL_SUCCESS_I32);
}

#[test]
fn doctor_returns_zero_when_no_pins_missing() {
    assert_eq!(doctor_exit_code(&totals(0, 0)), 0);
}

#[test]
fn doctor_returns_partial_success_when_any_pin_missing() {
    assert_eq!(doctor_exit_code(&totals(1, 0)), PARTIAL_SUCCESS_I32);
    assert_eq!(doctor_exit_code(&totals(50, 0)), PARTIAL_SUCCESS_I32);
}

#[test]
fn doctor_returns_partial_success_when_any_pin_stale() {
    assert_eq!(doctor_exit_code(&totals(0, 1)), PARTIAL_SUCCESS_I32);
}

#[test]
fn verify_all_returns_zero_when_all_clean() {
    assert_eq!(verify_all_exit_code(10, 0, 0).unwrap(), 0);
}

#[test]
fn verify_all_returns_zero_for_empty_store() {
    assert_eq!(verify_all_exit_code(0, 0, 0).unwrap(), 0);
}

#[test]
fn verify_all_returns_partial_success_for_stale_extractor_only() {
    assert_eq!(verify_all_exit_code(10, 0, 3).unwrap(), PARTIAL_SUCCESS_I32);
}

#[test]
fn verify_all_returns_partial_success_for_stale_with_zero_verified() {
    // The store may legitimately have only stale snapshots if every read
    // succeeded but every extractor version mismatched. This stays in
    // PartialSuccess (not Failure) because the loads succeeded.
    assert_eq!(verify_all_exit_code(0, 0, 5).unwrap(), PARTIAL_SUCCESS_I32);
}

#[test]
fn verify_all_returns_partial_success_when_some_failed_some_loaded() {
    assert_eq!(verify_all_exit_code(8, 2, 0).unwrap(), PARTIAL_SUCCESS_I32);
}

#[test]
fn verify_all_returns_error_when_every_snapshot_failed() {
    let err = verify_all_exit_code(0, 5, 0).unwrap_err();
    assert!(
        err.to_string().contains("every snapshot failed"),
        "unexpected error message: {err}"
    );
}

#[test]
fn backport_applicability_returns_zero_when_every_row_applies() {
    assert_eq!(backport_applicability_exit_code(0, 0), 0);
}

#[test]
fn backport_applicability_returns_partial_success_for_with_adaptation() {
    assert_eq!(backport_applicability_exit_code(1, 0), PARTIAL_SUCCESS_I32);
}

#[test]
fn backport_applicability_returns_partial_success_for_not_applicable() {
    assert_eq!(backport_applicability_exit_code(0, 1), PARTIAL_SUCCESS_I32);
}

#[test]
fn backport_applicability_returns_partial_success_for_mixed_attention_rows() {
    assert_eq!(backport_applicability_exit_code(3, 2), PARTIAL_SUCCESS_I32);
}
