// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::verdict::{InapplicableReason, TouchedKinds};

#[test]
fn flag_alone_yields_only_self_surface_touched() {
    let tk = TouchedKinds {
        only_self_surface: true,
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlySelfSurfaceTouched)
    );
}

#[test]
fn self_surface_dominates_over_docs_and_tests() {
    let tk = TouchedKinds {
        only_self_surface: true,
        erl: 3,
        docs: 1,
        tests: 2,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlySelfSurfaceTouched)
    );
}

#[test]
fn test_visibility_wins_over_self_surface_when_both_set() {
    let tk = TouchedKinds {
        only_test_visibility: true,
        only_self_surface: true,
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlyTestVisibilityChanged)
    );
}

#[test]
fn unset_flag_falls_through_to_existing_rules() {
    let tk = TouchedKinds {
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(tk.inapplicable_reason(), None);
}

#[test]
fn flag_alone_with_no_erl_still_returns_self_surface() {
    let tk = TouchedKinds {
        only_self_surface: true,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlySelfSurfaceTouched)
    );
}

#[test]
fn touched_kinds_is_not_empty_when_self_surface_flag_set() {
    let tk = TouchedKinds {
        only_self_surface: true,
        ..TouchedKinds::default()
    };
    assert!(!tk.is_empty(), "flag should keep TouchedKinds non-empty");
}

#[test]
fn as_str_returns_snake_case() {
    assert_eq!(
        InapplicableReason::OnlySelfSurfaceTouched.as_str(),
        "only_self_surface_touched"
    );
}
