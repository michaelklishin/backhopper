// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `apply_path_routing` rewrites a PinVerdict to Inapplicable when no
//! touched path is in the pin's scope, with TouchedKinds taking
//! precedence over the §3 fallback.

use backhopper_core::compat::routing::PathRouting;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{InapplicableReason, PinVerdict, TouchedKinds, Verdict};

use backhopper_cli::commands::check::apply_path_routing;

fn pin() -> Pin {
    Pin {
        project: ProjectName::new("demo").unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
    }
}

fn fresh_verdict() -> PinVerdict {
    PinVerdict::new(pin(), Verdict::Compatible)
}

#[test]
fn no_rewrite_when_any_path_is_in_scope() {
    let pv = fresh_verdict();
    let routing = PathRouting {
        any_in_scope: true,
        ..Default::default()
    };
    let out = apply_path_routing(pv.clone(), &routing);
    assert_eq!(out.verdict, pv.verdict);
}

#[test]
fn no_rewrite_when_no_attribution_exists() {
    let pv = fresh_verdict();
    let routing = PathRouting::default();
    let out = apply_path_routing(pv.clone(), &routing);
    assert_eq!(out.verdict, pv.verdict);
}

fn erlang_kinds() -> TouchedKinds {
    TouchedKinds {
        erl: 1,
        ..Default::default()
    }
}

#[test]
fn rewrites_to_out_of_scope_for_when_sibling_owns_every_path() {
    let pv = fresh_verdict().with_touched(erlang_kinds());
    let mut owners = std::collections::BTreeSet::new();
    owners.insert(ProjectName::new("cuttlefish").unwrap());
    let routing = PathRouting {
        any_in_scope: false,
        out_of_scope_owners: owners,
        untracked_count: 0,
    };
    let out = apply_path_routing(pv, &routing);
    match out.verdict {
        Verdict::Inapplicable {
            reason: InapplicableReason::OutOfScopeFor { project },
        } => assert_eq!(project.as_str(), "cuttlefish"),
        other => panic!("unexpected verdict: {other:?}"),
    }
}

#[test]
fn rewrites_to_untracked_when_no_owner_attributed() {
    let pv = fresh_verdict().with_touched(erlang_kinds());
    let routing = PathRouting {
        any_in_scope: false,
        out_of_scope_owners: Default::default(),
        untracked_count: 3,
    };
    let out = apply_path_routing(pv, &routing);
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
    let mut owners = std::collections::BTreeSet::new();
    owners.insert(ProjectName::new("cuttlefish").unwrap());
    let routing = PathRouting {
        any_in_scope: false,
        out_of_scope_owners: owners,
        untracked_count: 0,
    };
    let out = apply_path_routing(pv, &routing);
    assert!(matches!(
        out.verdict,
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyDocsTouched
        }
    ));
}
