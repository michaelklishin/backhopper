// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::config::ProjectFamily;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::verdict::Reason;

#[test]
fn ra_family_declares_log_segment_constants() {
    let d = ProjectFamily::Ra.defaults();
    let seg = d
        .wire_constants
        .iter()
        .find(|c| c.module == "ra_log_segment")
        .expect("ra_log_segment declared");
    assert!(seg.macros.iter().any(|m| m == "VERSION"));
    assert!(seg.macros.iter().any(|m| m == "MAGIC"));
}

#[test]
fn ra_family_declares_ra_machine_behaviour() {
    let d = ProjectFamily::Ra.defaults();
    assert!(
        d.versioned_machines
            .iter()
            .any(|v| v.behaviour == "ra_machine")
    );
}

#[test]
fn osiris_family_declares_log_version_constants() {
    let d = ProjectFamily::Osiris.defaults();
    let osiris = d
        .wire_constants
        .iter()
        .find(|c| c.module == "osiris")
        .expect("osiris constants declared");
    assert!(osiris.macros.iter().any(|m| m == "LOG_VERSION"));
    assert!(osiris.macros.iter().any(|m| m == "MAGIC"));
}

#[test]
fn khepri_family_declares_node_payload_versions() {
    let d = ProjectFamily::Khepri.defaults();
    let node = d
        .wire_constants
        .iter()
        .find(|c| c.module == "khepri_node")
        .expect("khepri_node constants declared");
    assert!(node.macros.iter().any(|m| m == "INIT_DATA_VERSION"));
}

#[test]
fn rabbitmq_family_declares_versioned_machine_impls() {
    let d = ProjectFamily::Rabbitmq.defaults();
    let fifo = d
        .versioned_machine_impls
        .iter()
        .find(|i| i.module == "rabbit_fifo")
        .expect("rabbit_fifo declared");
    assert!(fifo.allow_state_flag_gating);

    let stream = d
        .versioned_machine_impls
        .iter()
        .find(|i| i.module == "rabbit_stream_coordinator")
        .expect("rabbit_stream_coordinator declared");
    assert!(!stream.allow_state_flag_gating);
}

#[test]
fn generic_family_has_no_defaults() {
    let d = ProjectFamily::Generic.defaults();
    assert!(d.wire_constants.is_empty());
    assert!(d.versioned_machines.is_empty());
    assert!(d.versioned_machine_impls.is_empty());
}

#[test]
fn wire_constant_changed_serializes() {
    let r = Reason::WireConstantChanged {
        module: ModuleName::new("ra_log_segment").unwrap(),
        macro_name: "VERSION".into(),
        before: "2".into(),
        after: "3".into(),
    };
    let j = serde_json::to_string(&r).unwrap();
    assert!(j.contains("\"kind\":\"wire_constant_changed\""));
    let parsed: Reason = serde_json::from_str(&j).unwrap();
    assert_eq!(r, parsed);
}

#[test]
fn historical_implementation_missing_is_blocking() {
    let r = Reason::HistoricalImplementationMissing {
        module: ModuleName::new("rabbit_fifo").unwrap(),
        advertised_version_before: 7,
        advertised_version_after: 8,
        expected_historical_module: ModuleName::new("rabbit_fifo_v7").unwrap(),
    };
    assert!(r.is_blocking());
}

#[test]
fn wire_contract_body_drift_is_non_blocking() {
    let r = Reason::WireContractBodyDrift {
        module: ModuleName::new("rabbit_fifo").unwrap(),
        functions: vec![FunctionName::new("apply").unwrap()],
        advertised_version: 7,
    };
    assert!(!r.is_blocking());
}

#[test]
fn wire_contract_regression_is_blocking() {
    let r = Reason::WireContractRegression {
        module: ModuleName::new("rabbit_fifo").unwrap(),
        pin_version: 8,
        patch_version: 7,
    };
    assert!(r.is_blocking());
}

#[test]
fn arity_unused_warning_silenced() {
    // touch Arity so the import is not flagged unused
    let _ = Arity::new(0);
}
