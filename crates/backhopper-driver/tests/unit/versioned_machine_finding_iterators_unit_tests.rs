// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Driver-side typed-iterator accessors for
//! `Reason::VersionedMachineSnapshotMissing` and
//! `Reason::WireConstantBindingsMissing`. q-port and similar consumers
//! walk these iterators rather than re-deriving from the `Reason` enum
//! match.

use backhopper_driver::SeriesEvaluation;
use backhopper_driver::types::SnapshotSide;
use serde_json::json;

fn parse(value: serde_json::Value) -> SeriesEvaluation {
    serde_json::from_value(value).expect("SeriesEvaluation deserializes")
}

fn one_pin_with_reasons(reasons: serde_json::Value) -> serde_json::Value {
    json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.0",
            "pins": [ { "project": "rabbitmq-server", "tag": "v4.0.5" } ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "rabbitmq-server", "tag": "v4.0.5" },
                    "verdict": { "verdict": "requires_adaptation", "reasons": reasons }
                }
            ],
            "summary": { "compatible": 0, "requires_adaptation": 1, "incompatible": 0 }
        },
        "diagnostics": { "missing_test_modules": {} }
    })
}

#[test]
fn versioned_machine_snapshot_missing_iterator_finds_pinned_reason() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "versioned_machine_snapshot_missing",
            "module": "rabbit_fifo",
            "side": "both"
        }
    ])));
    let findings: Vec<_> = eval.versioned_machine_snapshot_missing().collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].module.as_str(), "rabbit_fifo");
    assert_eq!(findings[0].side, SnapshotSide::Both);
    assert_eq!(findings[0].pin.pin.project.as_str(), "rabbitmq-server");
}

#[test]
fn wire_constant_bindings_missing_iterator_extracts_macros_and_side() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "wire_constant_bindings_missing",
            "module": "ra_log_segment",
            "macros": ["MAGIC", "VERSION"],
            "side": "target"
        }
    ])));
    let findings: Vec<_> = eval.wire_constant_bindings_missing().collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].module.as_str(), "ra_log_segment");
    assert_eq!(findings[0].side, SnapshotSide::Target);
    let names: Vec<&str> = findings[0].macros.iter().map(|m| m.as_str()).collect();
    assert_eq!(names, vec!["MAGIC", "VERSION"]);
}

#[test]
fn compatible_pin_carries_no_findings() {
    let value = json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.0",
            "pins": [ { "project": "rabbitmq-server", "tag": "v4.0.5" } ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "rabbitmq-server", "tag": "v4.0.5" },
                    "verdict": { "verdict": "compatible" }
                }
            ],
            "summary": { "compatible": 1, "requires_adaptation": 0, "incompatible": 0 }
        },
        "diagnostics": { "missing_test_modules": {} }
    });
    let eval = parse(value);
    assert_eq!(eval.versioned_machine_snapshot_missing().count(), 0);
    assert_eq!(eval.wire_constant_bindings_missing().count(), 0);
}

#[test]
fn iterators_skip_unrelated_reason_variants() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "header_file_missing",
            "source_path": "deps/rabbit/src/x.erl",
            "include_directive": { "form": "include", "path": "ra.hrl" },
            "attempted_paths": []
        }
    ])));
    assert_eq!(eval.versioned_machine_snapshot_missing().count(), 0);
    assert_eq!(eval.wire_constant_bindings_missing().count(), 0);
}
