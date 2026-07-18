// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The clearance and verdict types reach a driver consumer through
//! `backhopper_driver::types`, and the wire-recorded `self_projects`
//! drives `clearance_self_inferred` and `tracked_refs`.

use backhopper_driver::CheckPayload;
use backhopper_driver::types::{
    BatchPayload, BumpStatus, BumpSummary, ClearanceFacts, Diagnostics, PatchFacts, PinBump,
    ReasonHistogram, RoundClearance, SeriesSummary, SourceDelta,
};
use serde_json::json;

// Naming every type through the façade guards the re-exports: drop one
// and this stops compiling.
#[allow(dead_code)]
struct NamesEveryReexport<'a> {
    a: &'a RoundClearance,
    b: &'a ClearanceFacts,
    c: &'a ReasonHistogram,
    d: &'a BumpSummary,
    e: &'a Diagnostics,
    f: &'a PatchFacts,
    g: &'a BumpStatus,
    h: &'a PinBump,
    i: &'a SeriesSummary,
    j: &'a SourceDelta,
}

fn batch_payload(self_projects: serde_json::Value) -> BatchPayload {
    let mut value = json!({
        "queried_against": [],
        "results": [
            {
                "commit": "a".repeat(40),
                "series": "v4.1.x",
                "verdict": {
                    "results": [
                        {
                            "pin": { "project": "rabbit", "tag": "v4.1.0" },
                            "verdict": { "verdict": "compatible" },
                            "tracked_refs": 9
                        },
                        {
                            "pin": { "project": "ra", "tag": "v2.16.7" },
                            "verdict": { "verdict": "compatible" },
                            "tracked_refs": 4
                        }
                    ],
                    "summary": { "compatible": 2, "requires_adaptation": 0, "incompatible": 0 }
                }
            }
        ]
    });
    if !self_projects.is_null() {
        value
            .as_object_mut()
            .unwrap()
            .insert("self_projects".into(), self_projects);
    }
    serde_json::from_value(value).expect("BatchPayload deserializes")
}

#[test]
fn round_clearance_matches_through_the_facade() {
    let payload = batch_payload(json!(["rabbit"]));
    let clearance = payload.clearance_self_inferred().expect("self set present");
    // The Findings arm carries the tracked `ra` reference.
    match clearance {
        RoundClearance::Findings(facts) => assert_eq!(facts.tracked, 4),
        RoundClearance::Clean(_) | RoundClearance::ZeroDomain(_) => {
            panic!("a tracked ra reference is a finding")
        }
    }
}

#[test]
fn self_inferred_is_none_for_a_pre_field_producer() {
    let payload = batch_payload(serde_json::Value::Null);
    assert_eq!(payload.self_projects, None);
    assert!(payload.clearance_self_inferred().is_none());
}

fn series_evaluation(self_projects: serde_json::Value) -> CheckPayload {
    let mut value = json!({
        "queried_against": { "kind": "series", "name": "v4.1.x", "pins": [] },
        "results": {
            "results": [
                {
                    "pin": { "project": "rabbit", "tag": "v4.1.0" },
                    "verdict": { "verdict": "compatible" },
                    "tracked_refs": 9
                },
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "compatible" },
                    "tracked_refs": 4
                }
            ],
            "summary": { "compatible": 2, "requires_adaptation": 0, "incompatible": 0 }
        },
        "diagnostics": {}
    });
    if !self_projects.is_null() {
        value
            .as_object_mut()
            .unwrap()
            .insert("self_projects".into(), self_projects);
    }
    serde_json::from_value(value).expect("CheckPayload deserializes")
}

#[test]
fn single_check_tracked_refs_excludes_self() {
    let evaluation = series_evaluation(json!(["rabbit"]));
    assert_eq!(evaluation.tracked_refs(), Some(4));
}

#[test]
fn single_check_tracked_refs_is_none_for_a_pre_field_producer() {
    let evaluation = series_evaluation(serde_json::Value::Null);
    assert!(evaluation.tracked_refs().is_none());
}
