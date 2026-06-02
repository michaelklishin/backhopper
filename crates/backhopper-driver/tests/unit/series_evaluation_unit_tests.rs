// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_driver::types::{ProjectName, TagName};
use backhopper_driver::{AggregateVerdict, SeriesEvaluation, VerdictKind};
use serde_json::json;

fn parse_evaluation(value: serde_json::Value) -> SeriesEvaluation {
    serde_json::from_value(value).expect("SeriesEvaluation deserializes")
}

#[test]
fn empty_series_yields_aggregate_empty() {
    let evaluation = parse_evaluation(json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": []
        },
        "results": {
            "results": [],
            "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0 }
        }
    }));
    assert_eq!(evaluation.worst_verdict(), AggregateVerdict::Empty);
    assert!(!evaluation.has_blocking_reason());
}

#[test]
fn mixed_series_picks_the_worst_verdict() {
    let evaluation = parse_evaluation(json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": [
                { "project": "ra", "tag": "v2.16.7" },
                { "project": "khepri", "tag": "v0.17.5" }
            ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "compatible" }
                },
                {
                    "pin": { "project": "khepri", "tag": "v0.17.5" },
                    "verdict": { "verdict": "incompatible", "reasons": [] }
                }
            ],
            "summary": { "compatible": 1, "requires_adaptation": 0, "incompatible": 1 }
        }
    }));
    assert_eq!(evaluation.worst_verdict(), AggregateVerdict::Incompatible);
    assert!(evaluation.has_blocking_reason());
}

#[test]
fn pins_in_filters_by_verdict_kind() {
    let evaluation = parse_evaluation(json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": [
                { "project": "ra", "tag": "v2.16.7" }
            ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "requires_adaptation", "reasons": [] }
                }
            ],
            "summary": { "compatible": 0, "requires_adaptation": 1, "incompatible": 0 }
        }
    }));
    assert_eq!(
        evaluation.pins_in(VerdictKind::RequiresAdaptation).count(),
        1
    );
    assert_eq!(evaluation.pins_in(VerdictKind::Compatible).count(), 0);
}

#[test]
fn pin_by_project_locates_matching_pin() {
    let evaluation = parse_evaluation(json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": [
                { "project": "ra", "tag": "v2.16.7" }
            ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "compatible" }
                }
            ],
            "summary": { "compatible": 1, "requires_adaptation": 0, "incompatible": 0 }
        }
    }));
    let project = ProjectName::from_str("ra").unwrap();
    let pin = evaluation.pin_by_project(&project).expect("pin present");
    assert_eq!(pin.pin.tag, TagName::from_str("v2.16.7").unwrap());
    let missing = ProjectName::from_str("ghost").unwrap();
    assert!(evaluation.pin_by_project(&missing).is_none());
}

#[test]
fn inapplicable_only_aggregates_to_inapplicable() {
    let evaluation = parse_evaluation(json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.2",
            "pins": [
                { "project": "ra", "tag": "v2.16.7" }
            ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "inapplicable", "reason": { "reason": "untracked" } }
                }
            ],
            "summary": { "compatible": 0, "requires_adaptation": 0, "incompatible": 0, "inapplicable": 1 }
        }
    }));
    assert_eq!(evaluation.worst_verdict(), AggregateVerdict::Inapplicable);
}
