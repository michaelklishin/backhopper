// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{Arity, FunctionName, Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{PinVerdict, SourceDelta, Verdict};

fn pin(project: &str, tag: &str) -> Pin {
    Pin {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
    }
}

fn mfa(module: &str, function: &str, arity: u8) -> Mfa {
    Mfa {
        module: ModuleName::new(module).unwrap(),
        function: FunctionName::new(function).unwrap(),
        arity: Arity::new(arity),
    }
}

#[test]
fn with_tracked_ref_details_populates_count_to_match_vec_len() {
    let pv =
        PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible).with_tracked_ref_details(vec![
            SymbolRef::function(mfa("ra", "start", 1)),
            SymbolRef::function(mfa("ra", "stop", 0)),
        ]);
    assert_eq!(pv.tracked_refs, 2);
    assert_eq!(pv.tracked_ref_details.len(), 2);
}

#[test]
fn pin_verdict_round_trips_tracked_ref_details_through_json() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible)
        .with_tracked_ref_details(vec![SymbolRef::function(mfa("ra", "start", 1))]);
    let json = serde_json::to_string(&pv).unwrap();
    let back: PinVerdict = serde_json::from_str(&json).unwrap();
    assert_eq!(pv, back);
}

#[test]
fn empty_tracked_ref_details_is_omitted_from_json() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible);
    let json = serde_json::to_string(&pv).unwrap();
    assert!(!json.contains("tracked_ref_details"), "{json}");
}

#[test]
fn non_empty_tracked_ref_details_appears_in_json() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible)
        .with_tracked_ref_details(vec![SymbolRef::function(mfa("ra", "start", 1))]);
    let json = serde_json::to_string(&pv).unwrap();
    assert!(json.contains("tracked_ref_details"), "{json}");
    assert!(json.contains("\"function\":\"start\""), "{json}");
}

#[test]
fn with_tracked_refs_does_not_clear_details_when_set_first() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible)
        .with_tracked_ref_details(vec![SymbolRef::function(mfa("ra", "x", 0))]);
    let after = pv.with_tracked_refs(42);
    assert_eq!(after.tracked_ref_details.len(), 1);
    assert_eq!(after.tracked_refs, 42);
}

#[test]
fn source_delta_details_round_trip_through_json() {
    let delta = SourceDelta {
        module: ModuleName::new("ra").unwrap(),
        function: FunctionName::new("start").unwrap(),
        arity: Arity::new(1),
        source_spec: "start(A :: integer()) -> ok".into(),
        target_spec: "start(A :: pos_integer()) -> ok".into(),
    };
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible)
        .with_source_delta_details(vec![delta]);
    let json = serde_json::to_string(&pv).unwrap();
    let back: PinVerdict = serde_json::from_str(&json).unwrap();
    assert_eq!(pv, back);
    assert!(json.contains("\"source_delta_details\""), "{json}");
    assert!(json.contains("\"source_spec\""), "{json}");
    assert!(json.contains("\"target_spec\""), "{json}");
}

#[test]
fn empty_source_delta_details_is_omitted_from_json() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible);
    let json = serde_json::to_string(&pv).unwrap();
    assert!(!json.contains("source_delta_details"), "{json}");
}

#[test]
fn pin_verdict_can_carry_both_tracked_refs_and_source_deltas() {
    let pv = PinVerdict::new(pin("ra", "v2.16.13"), Verdict::Compatible)
        .with_tracked_ref_details(vec![SymbolRef::function(mfa("ra", "x", 0))])
        .with_source_delta_details(vec![SourceDelta {
            module: ModuleName::new("ra").unwrap(),
            function: FunctionName::new("x").unwrap(),
            arity: Arity::new(0),
            source_spec: "x() -> ok".into(),
            target_spec: "x() -> {ok, term()}".into(),
        }]);
    assert_eq!(pv.tracked_refs, 1);
    assert_eq!(pv.tracked_ref_details.len(), 1);
    assert_eq!(pv.source_delta_details.len(), 1);
}
