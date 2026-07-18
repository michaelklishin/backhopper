// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `route_pin_verdict` is reason-granular: file ownership gates only
//! the path-scoped reason family, and a pin with in-scope reference
//! evidence keeps its verdict no matter which files the patch touched.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use backhopper_core::compat::routing::{PathRouting, route_pin_verdict};
use backhopper_core::model::names::{Mfa, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    InapplicableReason, PinVerdict, Reason, TouchedKinds, Verdict,
};

fn pin() -> Pin {
    Pin {
        project: ProjectName::new("demo").unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
    }
}

fn fresh_verdict() -> PinVerdict {
    PinVerdict::new(pin(), Verdict::Compatible)
}

fn erlang_kinds() -> TouchedKinds {
    TouchedKinds {
        erl: 1,
        ..Default::default()
    }
}

fn unowned_routing() -> PathRouting {
    let mut owners = BTreeSet::new();
    owners.insert(ProjectName::new("rabbitmq").unwrap());
    PathRouting {
        any_in_scope: false,
        out_of_scope_owners: owners,
        untracked_count: 0,
    }
}

fn missing_symbol_reason() -> Reason {
    Reason::MissingSymbol {
        symbol: SymbolRef::function(Mfa::from_str("cow_http:ensure_token/1").unwrap()),
        first_seen_at_tag: None,
        needs_pin_at_least: None,
        suggested_replacement: None,
    }
}

fn file_absent_reason() -> Reason {
    Reason::FileAbsent {
        path: PathBuf::from("src/other.erl"),
    }
}

#[test]
fn no_rewrite_when_any_path_is_in_scope() {
    let pv = fresh_verdict();
    let routing = PathRouting {
        any_in_scope: true,
        ..Default::default()
    };
    let out = route_pin_verdict(pv.clone(), &routing).into_inner();
    assert_eq!(out.verdict, pv.verdict);
}

#[test]
fn no_rewrite_when_no_attribution_exists() {
    let pv = fresh_verdict();
    let routing = PathRouting::default();
    let out = route_pin_verdict(pv.clone(), &routing).into_inner();
    assert_eq!(out.verdict, pv.verdict);
}

#[test]
fn rewrites_to_out_of_scope_for_when_sibling_owns_every_path() {
    let pv = fresh_verdict().with_touched(erlang_kinds());
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    match out.verdict {
        Verdict::Inapplicable {
            reason: InapplicableReason::OutOfScopeFor { project },
        } => assert_eq!(project.as_str(), "rabbitmq"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn rewrites_to_untracked_when_no_owner_attributed() {
    let pv = fresh_verdict().with_touched(erlang_kinds());
    let routing = PathRouting {
        any_in_scope: false,
        out_of_scope_owners: BTreeSet::new(),
        untracked_count: 3,
    };
    let out = route_pin_verdict(pv, &routing).into_inner();
    assert!(matches!(
        out.verdict,
        Verdict::Inapplicable {
            reason: InapplicableReason::Untracked
        }
    ));
}

#[test]
fn touched_kinds_signal_wins_over_routing_attribution() {
    let kinds = TouchedKinds {
        docs: 1,
        ..Default::default()
    };
    let pv = fresh_verdict().with_touched(kinds);
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    assert!(matches!(
        out.verdict,
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyDocsTouched
        }
    ));
}

// 2b645ce215: the pin owns no touched file, yet the unresolved snapshot call is kept through routing.
#[test]
fn reference_evidence_survives_for_unowned_pin() {
    let pv = PinVerdict::new(
        pin(),
        Verdict::Incompatible {
            reasons: vec![missing_symbol_reason()],
        },
    )
    .with_touched(erlang_kinds())
    .with_tracked_refs(1);
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    match out.verdict {
        Verdict::Incompatible { reasons } => assert_eq!(reasons.len(), 1),
        other => panic!("evidence was clobbered: {other:?}"),
    }
}

#[test]
fn compatible_with_tracked_refs_survives_for_unowned_pin() {
    let pv = fresh_verdict()
        .with_touched(erlang_kinds())
        .with_tracked_refs(2);
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    assert!(matches!(out.verdict, Verdict::Compatible));
}

#[test]
fn path_scoped_reasons_drop_for_unowned_pin() {
    let pv = PinVerdict::new(
        pin(),
        Verdict::Incompatible {
            reasons: vec![file_absent_reason()],
        },
    )
    .with_touched(erlang_kinds());
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    assert!(matches!(out.verdict, Verdict::Inapplicable { .. }));
}

#[test]
fn surviving_reasons_recompute_after_path_scoped_drop() {
    let pv = PinVerdict::new(
        pin(),
        Verdict::Incompatible {
            reasons: vec![file_absent_reason(), missing_symbol_reason()],
        },
    )
    .with_touched(erlang_kinds())
    .with_tracked_refs(1);
    let out = route_pin_verdict(pv, &unowned_routing()).into_inner();
    match out.verdict {
        Verdict::Incompatible { reasons } => {
            assert_eq!(reasons.len(), 1);
            assert!(matches!(reasons[0], Reason::MissingSymbol { .. }));
        }
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn inapplicable_pin_is_left_alone() {
    let pv = PinVerdict::new(
        pin(),
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyDocsTouched,
        },
    );
    let out = route_pin_verdict(pv.clone(), &unowned_routing()).into_inner();
    assert_eq!(out.verdict, pv.verdict);
}
