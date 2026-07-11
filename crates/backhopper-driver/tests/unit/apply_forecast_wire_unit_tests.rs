// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A v13 batch payload's `apply` field reaches a driver consumer:
//! `predicted_conflicts` reads the forecast on an all-inapplicable row
//! and the clearance folds it to `Findings`, the false green the
//! forecast exists to close. A pre-v13 payload without the field still
//! deserializes with `apply` absent.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_driver::types::{
    ApplyConflictKind, ApplyForecast, ApplyRollup, BatchPayload, PathApplyOutcome, RoundClearance,
    UnassessedReason,
};
use serde_json::json;

// Naming every apply-axis type through the façade guards the
// re-exports: drop one and this stops compiling.
#[allow(dead_code)]
struct NamesEveryReexport<'a> {
    a: &'a ApplyForecast,
    b: &'a PathApplyOutcome,
    c: &'a UnassessedReason,
    d: &'a ApplyRollup,
}

fn inapplicable_row(apply: serde_json::Value) -> serde_json::Value {
    let mut row = json!({
        "commit": "a".repeat(40),
        "series": "v4.0.x",
        "verdict": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
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
    if !apply.is_null() {
        row["apply"] = apply;
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
fn a_v13_forecast_reaches_predicted_conflicts_and_the_clearance() {
    let payload = payload_with(inapplicable_row(json!({
        "paths": {
            "deps/rabbit/test/quorum_queue_SUITE.erl": {
                "outcome": "conflict",
                "kind": "preimage_missing"
            },
            "deps/rabbit/src/rabbit_fifo.erl": { "outcome": "clean_exact" }
        }
    })));
    let predicted = payload.results[0].predicted_conflicts();
    assert_eq!(predicted.len(), 1);
    assert_eq!(
        predicted[0].path,
        PathBuf::from("deps/rabbit/test/quorum_queue_SUITE.erl")
    );
    assert_eq!(predicted[0].kind, ApplyConflictKind::PreimageMissing);
    let clearance = payload.clearance(&BTreeSet::new());
    assert!(matches!(clearance, RoundClearance::Findings(_)));
    assert_eq!(clearance.facts().apply.conflicted_rows, 1);
}

#[test]
fn a_pre_v13_row_deserializes_with_the_forecast_absent() {
    let payload = payload_with(inapplicable_row(serde_json::Value::Null));
    assert!(payload.results[0].apply.is_none());
    assert!(payload.results[0].predicted_conflicts().is_empty());
    let clearance = payload.clearance(&BTreeSet::new());
    assert!(matches!(clearance, RoundClearance::ZeroDomain(_)));
    assert_eq!(clearance.facts().apply.rows_with_forecast, 0);
}
