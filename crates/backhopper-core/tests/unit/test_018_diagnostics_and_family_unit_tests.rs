// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Diagnostics.missing_test_modules` (the always-on
//! diagnostic) and `FamilyDefaults.test_helper_search_paths` (per-family
//! override that drives the test-module resolver) shape and behaviour.

use backhopper_core::config::ProjectFamily;
use backhopper_core::model::names::{ModuleName, RelativePath};
use backhopper_core::model::verdict::Diagnostics;

fn mn(s: &str) -> ModuleName {
    ModuleName::new(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

#[test]
fn default_diagnostics_is_empty() {
    let d = Diagnostics::default();
    assert!(d.is_empty());
    assert!(d.missing_test_modules.is_empty());
}

#[test]
fn record_missing_test_module_creates_entry() {
    let mut d = Diagnostics::default();
    d.record_missing_test_module(rp("deps/rabbit/test/x_SUITE.erl"), mn("amqp_utils"));
    assert!(!d.is_empty());
    let inner = d
        .missing_test_modules
        .get(&rp("deps/rabbit/test/x_SUITE.erl"))
        .unwrap();
    assert_eq!(inner.get(&mn("amqp_utils")).copied(), Some(1));
}

#[test]
fn record_missing_test_module_increments_existing_count() {
    let mut d = Diagnostics::default();
    let suite = rp("deps/rabbit/test/x_SUITE.erl");
    d.record_missing_test_module(suite.clone(), mn("amqp_utils"));
    d.record_missing_test_module(suite.clone(), mn("amqp_utils"));
    d.record_missing_test_module(suite.clone(), mn("amqp_utils"));
    assert_eq!(
        d.missing_test_modules
            .get(&suite)
            .and_then(|i| i.get(&mn("amqp_utils")))
            .copied(),
        Some(3)
    );
}

#[test]
fn record_missing_test_module_groups_by_suite_and_helper() {
    let mut d = Diagnostics::default();
    d.record_missing_test_module(rp("deps/rabbit/test/a_SUITE.erl"), mn("h1"));
    d.record_missing_test_module(rp("deps/rabbit/test/a_SUITE.erl"), mn("h2"));
    d.record_missing_test_module(rp("deps/rabbit/test/b_SUITE.erl"), mn("h1"));
    assert_eq!(d.missing_test_modules.len(), 2);
    assert_eq!(
        d.missing_test_modules
            .get(&rp("deps/rabbit/test/a_SUITE.erl"))
            .map(|m| m.len()),
        Some(2)
    );
    assert_eq!(
        d.missing_test_modules
            .get(&rp("deps/rabbit/test/b_SUITE.erl"))
            .map(|m| m.len()),
        Some(1)
    );
}

#[test]
fn diagnostics_skips_serializing_when_missing_test_modules_empty() {
    let d = Diagnostics::default();
    let s = serde_json::to_string(&d).unwrap();
    assert!(!s.contains("missing_test_modules"));
}

#[test]
fn diagnostics_emits_missing_test_modules_when_populated() {
    let mut d = Diagnostics::default();
    d.record_missing_test_module(rp("deps/rabbit/test/x_SUITE.erl"), mn("amqp_utils"));
    let s = serde_json::to_string(&d).unwrap();
    assert!(s.contains("missing_test_modules"));
    assert!(s.contains("amqp_utils"));
}

#[test]
fn diagnostics_round_trips_through_serde() {
    let mut d = Diagnostics::default();
    d.record_missing_test_module(rp("deps/rabbit/test/x_SUITE.erl"), mn("amqp_utils"));
    let s = serde_json::to_string(&d).unwrap();
    let back: Diagnostics = serde_json::from_str(&s).unwrap();
    assert_eq!(d, back);
}

#[test]
fn rabbitmq_family_populates_test_helper_search_paths() {
    let defs = ProjectFamily::Rabbitmq.defaults();
    assert!(!defs.test_helper_search_paths.is_empty());
    assert!(
        defs.test_helper_search_paths
            .iter()
            .any(|p| p == "deps/*/test")
    );
    assert!(
        defs.test_helper_search_paths
            .iter()
            .any(|p| p == "deps/*/src")
    );
    assert!(
        defs.test_helper_search_paths
            .iter()
            .any(|p| p == "deps/rabbitmq_ct_helpers/src")
    );
}

#[test]
fn non_rabbitmq_families_leave_test_helper_search_paths_empty() {
    for f in [
        ProjectFamily::Generic,
        ProjectFamily::ErlangOtp,
        ProjectFamily::Ra,
        ProjectFamily::Osiris,
        ProjectFamily::Khepri,
    ] {
        let defs = f.defaults();
        assert!(
            defs.test_helper_search_paths.is_empty(),
            "family {f:?} should not opt in to test-helper resolution by default"
        );
    }
}

#[test]
fn rabbitmq_family_preserves_existing_wire_constants_and_machine_impls() {
    let defs = ProjectFamily::Rabbitmq.defaults();
    assert!(defs.wire_constants.is_empty());
    assert_eq!(defs.versioned_machine_impls.len(), 2);
    assert!(
        defs.versioned_machine_impls
            .iter()
            .any(|m| m.module == "rabbit_fifo")
    );
    assert!(
        defs.versioned_machine_impls
            .iter()
            .any(|m| m.module == "rabbit_stream_coordinator")
    );
}

#[test]
fn ra_family_preserves_wire_constants_after_test_helper_addition() {
    let defs = ProjectFamily::Ra.defaults();
    assert!(!defs.wire_constants.is_empty());
    assert!(defs.test_helper_search_paths.is_empty());
}
