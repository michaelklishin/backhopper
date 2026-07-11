// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `TargetFindings` accessors and wire shape.

use std::str::FromStr;

use backhopper_core::model::findings::TargetFindings;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use backhopper_core::model::resolver_coverage::ResolverClass;
use backhopper_core::model::verdict::{IndirectCallForm, Reason};

fn indirect_reason(function: &str, arity: u8) -> Reason {
    Reason::IndirectCallUndefinedOnTarget {
        source_path: RelativePath::new("deps/rabbit/test/maintenance_mode_SUITE.erl").unwrap(),
        module: ModuleName::from_str("rabbit_queue_type").unwrap(),
        function: FunctionName::from_str(function).unwrap(),
        arity: Arity::new(arity),
        via: IndirectCallForm::MeckExpect,
        line: 356,
    }
}

fn qualified_reason() -> Reason {
    Reason::QualifiedCallUndefinedOnTarget {
        source_path: RelativePath::new("deps/rabbit/src/rabbit_amqqueue.erl").unwrap(),
        module: ModuleName::from_str("rabbit_maintenance").unwrap(),
        function: FunctionName::from_str("status").unwrap(),
        arity: Arity::new(0),
        line: 42,
    }
}

fn classless_reason() -> Reason {
    Reason::TargetPathAbsent {
        path: RelativePath::new("deps/rabbit/priv/schema/rabbit.schema").unwrap(),
    }
}

#[test]
fn an_empty_record_reports_empty_and_nothing_else() {
    let findings = TargetFindings::default();
    assert!(findings.is_empty());
    assert!(findings.by_class().is_empty());
    assert_eq!(findings.unclassified(), 0);
    assert!(!findings.blocking());
}

#[test]
fn by_class_counts_per_resolver_class() {
    let findings = TargetFindings {
        reasons: vec![
            indirect_reason("drain", 1),
            indirect_reason("revive", 0),
            qualified_reason(),
        ],
    };
    assert_eq!(
        findings.by_class(),
        vec![
            (ResolverClass::QualifiedCall, 1),
            (ResolverClass::IndirectCall, 2),
        ]
    );
    assert_eq!(findings.unclassified(), 0);
}

#[test]
fn a_classless_reason_lands_in_the_unclassified_count() {
    let findings = TargetFindings {
        reasons: vec![indirect_reason("drain", 1), classless_reason()],
    };
    assert_eq!(findings.by_class(), vec![(ResolverClass::IndirectCall, 1)]);
    assert_eq!(findings.unclassified(), 1);
}

// every live-axis target reason is non-blocking, so the exit rule
// gates on non-empty, never on blocking
#[test]
fn target_axis_reasons_are_non_blocking() {
    let findings = TargetFindings {
        reasons: vec![indirect_reason("drain", 1), qualified_reason()],
    };
    assert!(!findings.blocking());
    assert!(!findings.is_empty());
}

#[test]
fn the_record_round_trips_through_json() {
    let findings = TargetFindings {
        reasons: vec![indirect_reason("drain", 1)],
    };
    let json = serde_json::to_value(&findings).unwrap();
    assert_eq!(
        json["reasons"][0]["kind"],
        "indirect_call_undefined_on_target"
    );
    let back: TargetFindings = serde_json::from_value(json).unwrap();
    assert_eq!(back, findings);
}

// Some-with-empty means assessed and clean; None means not evaluated
#[test]
fn some_empty_and_none_are_distinct_on_the_wire() {
    let some_empty = serde_json::to_value(Some(TargetFindings::default())).unwrap();
    assert_eq!(some_empty, serde_json::json!({ "reasons": [] }));
    let none: Option<TargetFindings> = None;
    assert_eq!(serde_json::to_value(none).unwrap(), serde_json::Value::Null);
}
