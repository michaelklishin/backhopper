// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Reason::resolver_class`: the predicted class a flag fell in. Not the
//! corpus break class, which is the symbol that actually broke.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::model::names::{
    Arity, FunctionName, GitRef, Mfa, ModuleName, RecordName, RelativePath, TypeName,
};
use backhopper_core::model::resolver_coverage::ResolverClass;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{IncludeDirective, Reason};

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

#[test]
fn the_target_tree_reasons_map_one_to_one() {
    let cases = [
        (
            Reason::MacroUndefinedOnTarget {
                source_path: rp("src/ra_server.erl"),
                macro_name: "RA_LOG".to_owned(),
                line: 1,
            },
            ResolverClass::Macro,
        ),
        (
            Reason::RecordUndefinedOnTarget {
                source_path: rp("src/ra_server.erl"),
                record_name: RecordName::from_str("state").unwrap(),
                line: 1,
            },
            ResolverClass::Record,
        ),
        (
            Reason::LocalCallUndefinedOnTarget {
                source_path: rp("src/ra_server.erl"),
                function: FunctionName::from_str("init_state").unwrap(),
                arity: Arity::new(1),
                line: 1,
            },
            ResolverClass::LocalCall,
        ),
        (
            Reason::QualifiedCallUndefinedOnTarget {
                source_path: rp("src/ra_server.erl"),
                module: module("rabbit_misc"),
                function: FunctionName::from_str("queue_resource").unwrap(),
                arity: Arity::new(2),
                line: 1,
            },
            ResolverClass::QualifiedCall,
        ),
        (
            Reason::HeaderFileMissing {
                source_path: rp("src/ra_server.erl"),
                include_directive: IncludeDirective::Include {
                    path: "rabbit.hrl".to_owned(),
                },
                attempted_paths: vec![rp("src/rabbit.hrl")],
            },
            ResolverClass::Include,
        ),
        (
            Reason::BehaviourModuleMissing {
                source_path: rp("src/ra_server.erl"),
                behaviour: module("ra_machine"),
            },
            ResolverClass::Behaviour,
        ),
        (
            Reason::MissingType {
                module: module("ra"),
                name: TypeName::from_str("server_id").unwrap(),
                arity: Arity::new(0),
            },
            ResolverClass::Type,
        ),
    ];
    for (reason, expected) in cases {
        assert_eq!(reason.resolver_class(), Some(expected));
    }
}

// The snapshot reasons carry a symbol, so their class follows its kind: a
// function is a qualified call, a macro stays a macro.
#[test]
fn snapshot_reasons_follow_the_symbol_kind() {
    let function = Reason::MissingSymbol {
        symbol: SymbolRef::function(Mfa::from_str("ra:members/1").unwrap()),
        first_seen_at_tag: None,
        needs_pin_at_least: None,
        suggested_replacement: None,
    };
    assert_eq!(
        function.resolver_class(),
        Some(ResolverClass::QualifiedCall)
    );

    let prereq = Reason::MissingPrereq {
        symbol: SymbolRef::macro_use("RA_LOG"),
        self_branch: GitRef::new("main").unwrap(),
        suggested_source_for_prereq: None,
    };
    assert_eq!(prereq.resolver_class(), Some(ResolverClass::Macro));
}

#[test]
fn a_non_symbol_reason_has_no_class() {
    let drift = Reason::PreimageMissing {
        path: "src/ra_server.erl".into(),
        hunk_index: 0,
        preimage_excerpt: String::new(),
    };
    assert_eq!(drift.resolver_class(), None);
}

// The drift guard: every class is reachable, so the taxonomy and the
// reasons cannot fall out of sync.
#[test]
fn every_resolver_class_is_reachable_from_some_reason() {
    let reasons = [
        Reason::MacroUndefinedOnTarget {
            source_path: rp("src/ra_server.erl"),
            macro_name: "RA_LOG".to_owned(),
            line: 1,
        },
        Reason::RecordUndefinedOnTarget {
            source_path: rp("src/ra_server.erl"),
            record_name: RecordName::from_str("state").unwrap(),
            line: 1,
        },
        Reason::LocalCallUndefinedOnTarget {
            source_path: rp("src/ra_server.erl"),
            function: FunctionName::from_str("init_state").unwrap(),
            arity: Arity::new(1),
            line: 1,
        },
        Reason::HeaderFileMissing {
            source_path: rp("src/ra_server.erl"),
            include_directive: IncludeDirective::Include {
                path: "defs.hrl".to_owned(),
            },
            attempted_paths: vec![],
        },
        Reason::BehaviourModuleMissing {
            source_path: rp("src/ra_server.erl"),
            behaviour: module("ra_machine"),
        },
        Reason::MissingType {
            module: module("ra"),
            name: TypeName::from_str("server_id").unwrap(),
            arity: Arity::new(0),
        },
        Reason::MissingSymbol {
            symbol: SymbolRef::function(Mfa::from_str("ra:members/1").unwrap()),
            first_seen_at_tag: None,
            needs_pin_at_least: None,
            suggested_replacement: None,
        },
    ];
    let reached: BTreeSet<ResolverClass> =
        reasons.iter().filter_map(Reason::resolver_class).collect();
    let all: BTreeSet<ResolverClass> = ResolverClass::ALL.into_iter().collect();
    assert_eq!(reached, all);
}
