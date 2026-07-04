// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! JSON wire-format lockdown for the OutOfScopeFor and Untracked
//! `InapplicableReason` variants.

use serde_json::json;

use backhopper_core::model::names::ProjectName;
use backhopper_core::model::verdict::{InapplicableReason, Verdict};

#[test]
fn out_of_scope_for_serialises_with_project_payload() {
    let v = Verdict::Inapplicable {
        reason: InapplicableReason::OutOfScopeFor {
            project: ProjectName::new("cuttlefish").unwrap(),
        },
    };
    let actual = serde_json::to_value(&v).unwrap();
    assert_eq!(
        actual,
        json!({
            "verdict": "inapplicable",
            "reason": {
                "reason": "out_of_scope_for",
                "project": "cuttlefish",
            },
        })
    );
}

#[test]
fn untracked_serialises_as_payload_free_tag() {
    let v = Verdict::Inapplicable {
        reason: InapplicableReason::Untracked,
    };
    let actual = serde_json::to_value(&v).unwrap();
    assert_eq!(
        actual,
        json!({
            "verdict": "inapplicable",
            "reason": { "reason": "untracked" },
        })
    );
}

#[test]
fn out_of_scope_for_round_trips_through_serde() {
    let original = InapplicableReason::OutOfScopeFor {
        project: ProjectName::new("rabbit").unwrap(),
    };
    let s = serde_json::to_string(&original).unwrap();
    let parsed: InapplicableReason = serde_json::from_str(&s).unwrap();
    assert_eq!(original, parsed);
}

#[test]
fn untracked_round_trips_through_serde() {
    let original = InapplicableReason::Untracked;
    let s = serde_json::to_string(&original).unwrap();
    let parsed: InapplicableReason = serde_json::from_str(&s).unwrap();
    assert_eq!(original, parsed);
}

#[test]
fn as_str_keys_match_serde_tag_strings() {
    assert_eq!(
        InapplicableReason::OutOfScopeFor {
            project: ProjectName::new("x").unwrap()
        }
        .as_str(),
        "out_of_scope_for"
    );
    assert_eq!(InapplicableReason::Untracked.as_str(), "untracked");
}
