// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::classify_visibility;
use backhopper_core::model::names::ModuleName;
use backhopper_core::model::snapshot::Visibility;

fn module(name: &str) -> ModuleName {
    ModuleName::new(name.to_owned()).expect("valid module name")
}

#[test]
fn plain_module_is_public() {
    let v = classify_visibility(&module("ra_server"), false, false, &[], &[]);
    assert_eq!(v, Visibility::Public);
}

#[test]
fn internal_listed_module_is_hidden() {
    let internal = vec!["ra_log_segment".to_owned()];
    let v = classify_visibility(&module("ra_log_segment"), false, false, &[], &internal);
    assert_eq!(v, Visibility::Hidden);
}

#[test]
fn hidden_hint_marks_module_hidden() {
    let v = classify_visibility(&module("khepri_machine"), true, false, &[], &[]);
    assert_eq!(v, Visibility::Hidden);
}

#[test]
fn public_listing_overrides_hidden_hint() {
    let public = vec!["khepri_machine".to_owned()];
    let v = classify_visibility(&module("khepri_machine"), true, false, &public, &[]);
    assert_eq!(v, Visibility::Public);
}

#[test]
fn test_only_exports_make_module_test_only() {
    let v = classify_visibility(&module("rabbit_ct_helpers"), false, true, &[], &[]);
    assert_eq!(v, Visibility::TestOnly);
}

#[test]
fn internal_listing_wins_over_test_only() {
    let internal = vec!["ra_test_internal".to_owned()];
    let v = classify_visibility(&module("ra_test_internal"), false, true, &[], &internal);
    assert_eq!(v, Visibility::Hidden);
}

#[test]
fn hidden_hint_wins_over_test_only() {
    let v = classify_visibility(&module("khepri_import"), true, true, &[], &[]);
    assert_eq!(v, Visibility::Hidden);
}
