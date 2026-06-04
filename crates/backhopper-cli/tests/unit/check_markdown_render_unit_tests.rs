// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_cli::commands::check::render_markdown_triage;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PinVerdict, Reason, SeriesEvaluation, SeriesVerdict, Verdict,
};

fn pin(project: &str, tag: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new(tag).unwrap(),
    )
}

fn build_eval(verdicts: Vec<PinVerdict>) -> SeriesEvaluation {
    SeriesEvaluation {
        verdict: SeriesVerdict::from_results(verdicts),
        diagnostics: Diagnostics::default(),
        patch_facts: Default::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
    }
}

fn render(eval: &SeriesEvaluation) -> String {
    let mut buf = Vec::new();
    render_markdown_triage(&mut buf, eval).expect("markdown renders");
    String::from_utf8(buf).expect("utf-8")
}

#[test]
fn header_row_present() {
    let eval = build_eval(vec![PinVerdict::new(
        pin("ra", "v3.1.7"),
        Verdict::Compatible,
    )]);
    let s = render(&eval);
    assert!(s.starts_with("| Pin | Verdict | Tracked refs | Notes |\n"));
    assert!(s.contains("| --- | --- | --- | --- |\n"));
}

#[test]
fn compatible_pin_renders_em_dash_in_notes() {
    let eval = build_eval(vec![PinVerdict::new(
        pin("ra", "v3.1.7"),
        Verdict::Compatible,
    )]);
    let s = render(&eval);
    assert!(s.contains("| ra@v3.1.7 | Compatible | 0 | — |"));
}

#[test]
fn inapplicable_pin_carries_reason_label() {
    let eval = build_eval(vec![PinVerdict::new(
        pin("ra", "v3.1.7"),
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlySchemaTouched,
        },
    )]);
    let s = render(&eval);
    assert!(s.contains("Inapplicable (only_schema_touched)"));
}

#[test]
fn incompatible_pin_lists_reasons() {
    let reason = Reason::MissingSymbol {
        symbol: SymbolRef::function(
            backhopper_core::model::names::Mfa::from_str("rabbit:foo/2").unwrap(),
        ),
        first_seen_at_tag: None,
        suggested_replacement: None,
    };
    let eval = build_eval(vec![PinVerdict::new(
        pin("rabbit", "v4.1.x"),
        Verdict::Incompatible {
            reasons: vec![reason],
        },
    )]);
    let s = render(&eval);
    assert!(s.contains("Incompatible"));
    assert!(s.contains("MissingSymbol rabbit:foo/2"));
}

#[test]
fn multiple_reasons_are_separated_by_semicolons() {
    let reasons = vec![
        Reason::FileAbsent {
            path: "deps/rabbit/src/x.erl".into(),
        },
        Reason::ModuleRelocated {
            module: ModuleName::new("rabbit_amqp_reader").unwrap(),
            patch_path: "deps/rabbit/src/rabbit_amqp_reader.erl".into(),
        },
    ];
    let eval = build_eval(vec![PinVerdict::new(
        pin("rabbit", "v3.13.x"),
        Verdict::Incompatible { reasons },
    )]);
    let s = render(&eval);
    assert!(s.contains("FileAbsent deps/rabbit/src/x.erl; ModuleRelocated rabbit_amqp_reader"));
}

#[test]
fn behaviour_callback_reasons_render() {
    let reasons = vec![
        Reason::BehaviourCallbackSignatureChanged {
            behaviour: ModuleName::new("bq").unwrap(),
            callback: FunctionName::new("init").unwrap(),
            arity: Arity::new(1),
            expected_after_patch: "init(A) -> {ok, state()}".into(),
            implementer: ModuleName::new("bq_impl").unwrap(),
            implementer_signature: "init(A) -> state()".into(),
        },
        Reason::BehaviourCallbackAdded {
            behaviour: ModuleName::new("bq").unwrap(),
            callback: FunctionName::new("recover").unwrap(),
            arity: Arity::new(2),
            implementer: ModuleName::new("bq_impl").unwrap(),
        },
    ];
    let eval = build_eval(vec![PinVerdict::new(
        pin("rabbit", "v4.2.x"),
        Verdict::Incompatible { reasons },
    )]);
    let s = render(&eval);
    assert!(s.contains("CallbackSignatureChanged bq:init/1"));
    assert!(s.contains("CallbackAdded bq:recover/2"));
}
