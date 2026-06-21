// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_core::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName, TypeName};
use backhopper_core::model::resolver_coverage::{ResolverClass, ResolverCoverage};
use backhopper_core::model::symbol::SymbolKind;

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

#[test]
fn every_class_is_covered() {
    assert!(
        ResolverClass::ALL
            .into_iter()
            .all(ResolverClass::is_covered)
    );
}

#[test]
fn class_round_trips_through_serde() {
    let s = serde_json::to_string(&ResolverClass::Macro).unwrap();
    assert_eq!(s, "\"macro\"");
    let back: ResolverClass = serde_json::from_str(&s).unwrap();
    assert_eq!(back, ResolverClass::Macro);
}

// One class per kind, including the `Type` namespace, so a type is never
// labeled a qualified call.
#[test]
fn of_symbol_kind_maps_each_kind() {
    let cases = [
        (
            SymbolKind::Macro {
                name: "FOO".to_owned(),
            },
            ResolverClass::Macro,
        ),
        (
            SymbolKind::Record {
                name: RecordName::from_str("state").unwrap(),
            },
            ResolverClass::Record,
        ),
        (
            SymbolKind::Behaviour {
                module: module("gen_server"),
            },
            ResolverClass::Behaviour,
        ),
        (
            SymbolKind::Callback {
                module: module("gen_server"),
                function: FunctionName::from_str("init").unwrap(),
                arity: Arity::new(1),
            },
            ResolverClass::Behaviour,
        ),
        (
            SymbolKind::Function {
                mfa: Mfa::from_str("ra:members/1").unwrap(),
            },
            ResolverClass::QualifiedCall,
        ),
        (
            SymbolKind::FunctionAnyArity {
                module: module("ra"),
                function: FunctionName::from_str("members").unwrap(),
            },
            ResolverClass::QualifiedCall,
        ),
        (
            SymbolKind::Type {
                module: module("ra"),
                name: TypeName::from_str("server_id").unwrap(),
                arity: Arity::new(0),
            },
            ResolverClass::Type,
        ),
    ];
    for (kind, expected) in cases {
        assert_eq!(ResolverClass::of_symbol_kind(&kind), expected);
    }
}

#[test]
fn current_coverage_checks_every_class_and_leaves_none_unchecked() {
    let cov = ResolverCoverage::current();
    assert!(ResolverClass::ALL.into_iter().all(|c| cov.is_checked(c)));
    assert_eq!(cov.unchecked().count(), 0);
}

// `unchecked` is exactly `ALL \ checked`, so a class can never be in both.
#[test]
fn unchecked_is_the_complement_of_checked() {
    let cov: ResolverCoverage = serde_json::from_str(r#"{"checked":["macro","record"]}"#).unwrap();
    assert!(cov.is_checked(ResolverClass::Macro));
    assert!(cov.is_checked(ResolverClass::Record));
    assert!(!cov.is_checked(ResolverClass::LocalCall));
    let unchecked: Vec<_> = cov.unchecked().collect();
    assert_eq!(unchecked.len(), ResolverClass::ALL.len() - 2);
    assert!(unchecked.contains(&ResolverClass::LocalCall));
    assert!(!unchecked.contains(&ResolverClass::Macro));
}

#[test]
fn coverage_round_trips_through_serde() {
    let cov = ResolverCoverage::current();
    let json = serde_json::to_string(&cov).unwrap();
    let back: ResolverCoverage = serde_json::from_str(&json).unwrap();
    assert_eq!(back, cov);
}
