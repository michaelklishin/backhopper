// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `apply_symbol_availability` populates the bump-first fields on
//! missing-symbol and arity-changed reasons and reclassifies the
//! affected pins: `is_blocking` is field-sensitive.

use std::str::FromStr;

use backhopper_core::model::names::{Arity, FunctionName, Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    AvailabilityQuery, Diagnostics, PatchFacts, PinVerdict, Reason, SeriesEvaluation,
    SeriesVerdict, Verdict,
};

fn pin(project: &str, tag: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new(tag).unwrap(),
    )
}

fn missing(symbol: &str) -> Reason {
    Reason::MissingSymbol {
        symbol: SymbolRef::function(Mfa::from_str(symbol).unwrap()),
        first_seen_at_tag: None,
        needs_pin_at_least: None,
        suggested_replacement: None,
    }
}

fn arity_changed() -> Reason {
    Reason::ArityChanged {
        module: ModuleName::new("dep_mod").unwrap(),
        function: FunctionName::new("multi").unwrap(),
        expected: Arity::new(4),
        found: vec![Arity::new(2)],
        expected_available_at: None,
        needs_pin_at_least: None,
    }
}

fn evaluation(pins: Vec<PinVerdict>) -> SeriesEvaluation {
    SeriesEvaluation {
        verdict: SeriesVerdict::from_results(pins),
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        apply: None,
        target_findings: None,
    }
}

#[test]
fn missing_symbol_reclassifies_to_requires_adaptation_when_available_later() {
    let mut eval = evaluation(vec![PinVerdict::new(
        pin("cowlib", "2.13.0"),
        Verdict::Incompatible {
            reasons: vec![missing("cow_http:ensure_token/1")],
        },
    )]);
    eval.apply_symbol_availability(|_, _| Some(TagName::new("2.17.0").unwrap()));
    let pv = &eval.verdict.results[0];
    match &pv.verdict {
        Verdict::RequiresAdaptation { reasons } => match &reasons[0] {
            Reason::MissingSymbol {
                first_seen_at_tag,
                needs_pin_at_least,
                ..
            } => {
                assert_eq!(first_seen_at_tag.as_ref().unwrap().as_str(), "2.17.0");
                let needs = needs_pin_at_least.as_ref().unwrap();
                assert_eq!(needs.project.as_str(), "cowlib");
                assert_eq!(needs.tag.as_str(), "2.17.0");
            }
            other => panic!("unexpected reason: {other:?}"),
        },
        other => panic!("expected requires_adaptation, got {other:?}"),
    }
    assert_eq!(eval.verdict.summary.requires_adaptation, 1);
    assert_eq!(eval.verdict.summary.incompatible, 0);
}

#[test]
fn missing_symbol_stays_incompatible_when_absent_everywhere() {
    let mut eval = evaluation(vec![PinVerdict::new(
        pin("cowlib", "2.13.0"),
        Verdict::Incompatible {
            reasons: vec![missing("cow_http:ensure_token/1")],
        },
    )]);
    eval.apply_symbol_availability(|_, _| None);
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Incompatible { .. }
    ));
    assert_eq!(eval.verdict.summary.incompatible, 1);
}

#[test]
fn arity_changed_gets_the_symmetric_treatment() {
    let mut eval = evaluation(vec![PinVerdict::new(
        pin("cowboy", "2.12.0"),
        Verdict::Incompatible {
            reasons: vec![arity_changed()],
        },
    )]);
    eval.apply_symbol_availability(|_, query| match query {
        AvailabilityQuery::ExactArity { arity, .. } if *arity == Arity::new(4) => {
            Some(TagName::new("2.16.0").unwrap())
        }
        _ => None,
    });
    match &eval.verdict.results[0].verdict {
        Verdict::RequiresAdaptation { reasons } => match &reasons[0] {
            Reason::ArityChanged {
                expected_available_at,
                needs_pin_at_least,
                ..
            } => {
                assert_eq!(expected_available_at.as_ref().unwrap().as_str(), "2.16.0");
                assert_eq!(needs_pin_at_least.as_ref().unwrap().tag.as_str(), "2.16.0");
            }
            other => panic!("unexpected reason: {other:?}"),
        },
        other => panic!("expected requires_adaptation, got {other:?}"),
    }
}

// A pin with one bump-satisfiable miss and one genuine miss must stay
// incompatible: the bump alone does not make the pick land.
#[test]
fn mixed_reasons_keep_the_blocking_classification() {
    let mut eval = evaluation(vec![PinVerdict::new(
        pin("cowlib", "2.13.0"),
        Verdict::Incompatible {
            reasons: vec![
                missing("cow_http:ensure_token/1"),
                missing("cow_http:gone_forever/1"),
            ],
        },
    )]);
    eval.apply_symbol_availability(|_, query| match query {
        AvailabilityQuery::ExactArity { function, .. } if function.as_str() == "ensure_token" => {
            Some(TagName::new("2.17.0").unwrap())
        }
        _ => None,
    });
    assert!(matches!(
        eval.verdict.results[0].verdict,
        Verdict::Incompatible { .. }
    ));
}

#[test]
fn compatible_and_inapplicable_pins_are_not_probed() {
    let mut probed = 0usize;
    let mut eval = evaluation(vec![PinVerdict::new(
        pin("cowlib", "2.13.0"),
        Verdict::Compatible,
    )]);
    eval.apply_symbol_availability(|_, _| {
        probed += 1;
        None
    });
    assert_eq!(probed, 0);
}

#[test]
fn lookup_receives_the_owning_pin() {
    let mut eval = evaluation(vec![
        PinVerdict::new(
            pin("cowboy", "2.12.0"),
            Verdict::Incompatible {
                reasons: vec![missing("cowboy_constraints:from_fun/1")],
            },
        ),
        PinVerdict::new(
            pin("cowlib", "2.13.0"),
            Verdict::Incompatible {
                reasons: vec![missing("cow_http:ensure_token/1")],
            },
        ),
    ]);
    let mut seen: Vec<String> = Vec::new();
    eval.apply_symbol_availability(|pin, _| {
        seen.push(pin.project.to_string());
        None
    });
    assert_eq!(seen, vec!["cowboy".to_owned(), "cowlib".to_owned()]);
}
