// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_cli::commands::check::render_markdown_triage;
use backhopper_core::model::names::DependencyName;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, InapplicableReason, PinBump, PinVerdict, Reason, SeriesEvaluation,
    SeriesVerdict, Verdict,
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
        needs_pin_at_least: None,
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

fn eval_with_pin_bump(status: Option<BumpStatus>) -> SeriesEvaluation {
    let mut eval = build_eval(vec![PinVerdict::new(
        pin("ra", "v3.1.7"),
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyMakefileTouched,
        },
    )]);
    eval.diagnostics.pin_bumps.push(PinBump {
        dep: DependencyName::new("cowlib".to_owned()).unwrap(),
        from: Some("hex 2.16.0".into()),
        to: "hex 2.17.1".into(),
        status,
    });
    eval
}

#[test]
fn pin_bump_line_precedes_the_triage_table() {
    let s = render(&eval_with_pin_bump(Some(BumpStatus::SnapshotMissing {
        note: "run: backhopper snapshots generate --project cowlib --since 2.17.1".into(),
    })));
    let bump_line = s.lines().next().unwrap();
    assert!(bump_line.starts_with("**Pin bump:** cowlib hex 2.16.0 -> hex 2.17.1"));
    assert!(bump_line.contains("snapshot: absent"), "{bump_line}");
    assert!(bump_line.contains("snapshots generate"), "{bump_line}");
    // blank separator keeps the markdown table valid
    assert!(s.contains("\n\n| Pin |"), "{s}");
}

#[test]
fn present_and_untracked_statuses_render_their_labels() {
    let present = render(&eval_with_pin_bump(Some(BumpStatus::SnapshotPresent)));
    assert!(present.contains("(snapshot: present)"), "{present}");
    let untracked = render(&eval_with_pin_bump(Some(BumpStatus::Untracked)));
    assert!(untracked.contains("(untracked dep)"), "{untracked}");
    let unassessed = render(&eval_with_pin_bump(None));
    assert!(unassessed.contains("(unassessed)"), "{unassessed}");
}

#[test]
fn no_pin_bumps_means_no_bump_line_and_no_leading_blank() {
    let eval = build_eval(vec![PinVerdict::new(
        pin("ra", "v3.1.7"),
        Verdict::Compatible,
    )]);
    let s = render(&eval);
    assert!(!s.contains("Pin bump"));
    assert!(s.starts_with("| Pin |"));
}
