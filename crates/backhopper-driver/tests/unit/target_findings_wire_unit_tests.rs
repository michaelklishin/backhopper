// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A v14 batch payload's `target_findings` field reaches a driver
//! consumer: an all-inapplicable row with a finding still folds the
//! clearance to `Findings`, the false green the field exists to close.
//! A pre-v14 payload without the field still deserializes with
//! `target_findings` absent.

use std::collections::BTreeSet;

use backhopper_driver::types::{
    BatchPayload, RoundClearance, TargetFindings, TargetFindingsRollup,
};
use serde_json::json;

// Naming the symbol-axis types through the façade guards the
// re-exports: drop one and this stops compiling.
#[allow(dead_code)]
struct NamesEveryReexport<'a> {
    a: &'a TargetFindings,
    b: &'a TargetFindingsRollup,
}

fn inapplicable_row(target_findings: serde_json::Value) -> serde_json::Value {
    let mut row = json!({
        "commit": "a".repeat(40),
        "series": "v3.13.x",
        "verdict": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.15.3" },
                    "verdict": {
                        "verdict": "inapplicable",
                        "reason": { "reason": "only_test_fixtures_touched" }
                    }
                }
            ],
            "summary": {
                "compatible": 0,
                "requires_adaptation": 0,
                "incompatible": 0,
                "inapplicable": 1
            }
        }
    });
    if !target_findings.is_null() {
        row["target_findings"] = target_findings;
    }
    row
}

fn payload_with(row: serde_json::Value) -> BatchPayload {
    serde_json::from_value(json!({
        "queried_against": [],
        "results": [row],
        "self_projects": []
    }))
    .expect("payload deserializes")
}

#[test]
fn a_v14_finding_reaches_the_clearance() {
    let payload = payload_with(inapplicable_row(json!({
        "reasons": [
            {
                "kind": "indirect_call_undefined_on_target",
                "source_path": "deps/rabbit/test/maintenance_mode_SUITE.erl",
                "module": "rabbit_queue_type",
                "function": "drain",
                "arity": 1,
                "via": "meck_expect",
                "line": 356
            }
        ]
    })));
    let findings = payload.results[0]
        .target_findings
        .as_ref()
        .expect("field present");
    assert!(!findings.is_empty());
    let clearance = payload.clearance(&BTreeSet::new());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    assert_eq!(clearance.facts().target.finding_rows, 1);
    assert_eq!(clearance.facts().exit_code, 3);
}

#[test]
fn a_pre_v14_row_deserializes_with_the_findings_absent() {
    let payload = payload_with(inapplicable_row(serde_json::Value::Null));
    assert!(payload.results[0].target_findings.is_none());
    let clearance = payload.clearance(&BTreeSet::new());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    assert_eq!(clearance.facts().target.rows_with_findings, 0);
}
