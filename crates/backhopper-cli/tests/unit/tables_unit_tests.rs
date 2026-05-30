// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};
use backhopper_core::model::verdict::{
    Diagnostics, PinVerdict, Reason, SeriesEvaluation, SeriesVerdict, Verdict,
};
use time::OffsetDateTime;

use bel7_cli::TableStyle;

use backhopper_cli::tables::render_evaluation_table;

const ALL_TABLE_STYLES: &[TableStyle] = &[
    TableStyle::Modern,
    TableStyle::Borderless,
    TableStyle::Markdown,
    TableStyle::Sharp,
    TableStyle::Ascii,
    TableStyle::Psql,
    TableStyle::Dots,
];

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src/**/*.erl".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
    }
}

fn pin_for(project: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn snapshot_with_module(
    project: &str,
    mod_name: &str,
    exports: &[(&str, u8)],
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new(mod_name).unwrap());
    m.visibility = Visibility::Public;
    for (f, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*f).unwrap(),
            arity: Arity::new(*a),
        });
    }
    Snapshot::from_extracted(header(project), vec![m], vec![]).into_canonical()
}

#[test]
fn table_contains_pin_and_verdict_columns() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("ra@v1.0.0"), "table: {text}");
    assert!(text.contains("Compatible"), "table: {text}");
    assert!(
        text.contains("1 tracked symbol referenced"),
        "table: {text}"
    );
}

#[test]
fn table_lists_missing_symbol_reason_with_mfa_detail() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:gone(1, 2, 3).
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("Incompatible"), "table: {text}");
    assert!(text.contains("MissingSymbol"), "table: {text}");
    assert!(text.contains("ra:gone/3"), "table: {text}");
}

#[test]
fn table_handles_synthetic_record_fields_changed_reason() {
    let eval = SeriesEvaluation {
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(
            pin_for("demo"),
            Verdict::Incompatible {
                reasons: vec![
                    Reason::RecordFieldsChanged {
                        record: RecordName::new("user").unwrap(),
                        expected: vec![],
                        found: vec![],
                    },
                    Reason::FileAbsent {
                        path: PathBuf::from("src/a.erl"),
                    },
                ],
            },
        )]),
        diagnostics: Diagnostics::default(),
        patch_facts: Default::default(),
    };
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("RecordFields"), "table: {text}");
    assert!(text.contains("FileAbsent"), "table: {text}");
    assert!(text.contains("src/a.erl"), "table: {text}");
}

#[test]
fn every_bel7_table_style_renders_pin_and_verdict() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    for style in ALL_TABLE_STYLES {
        let text = render_evaluation_table(&eval, *style);
        assert!(
            text.contains("ra@v1.0.0"),
            "style {style:?} dropped pin column: {text}"
        );
        assert!(
            text.contains("Compatible"),
            "style {style:?} dropped verdict column: {text}"
        );
    }
}

#[test]
fn markdown_style_uses_pipe_separators() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let md = render_evaluation_table(&eval, TableStyle::Markdown);
    assert!(md.contains('|'), "markdown table should use pipes: {md}");
    assert!(
        md.contains("|---") || md.contains("|--"),
        "markdown table should have header separator: {md}"
    );
}

#[test]
fn clause_mismatch_reason_renders_call_args_and_pin_clauses() {
    let eval = SeriesEvaluation {
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(
            pin_for("ra"),
            Verdict::Incompatible {
                reasons: vec![Reason::ClauseMismatch {
                    module: ModuleName::new("ra").unwrap(),
                    function: FunctionName::new("mode").unwrap(),
                    arity: Arity::new(1),
                    call_args: vec![ArgShape::Atom {
                        name: "restart".into(),
                    }],
                    pin_clauses: vec![
                        vec![ArgShape::Atom {
                            name: "start".into(),
                        }],
                        vec![ArgShape::Atom {
                            name: "stop".into(),
                        }],
                    ],
                }],
            },
        )]),
        diagnostics: Diagnostics::default(),
        patch_facts: Default::default(),
    };
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("ClauseMismatch"), "table: {text}");
    assert!(text.contains("ra:mode/1"), "table: {text}");
    assert!(text.contains("'restart'"), "table: {text}");
    assert!(text.contains("'start'"), "table: {text}");
    assert!(text.contains("'stop'"), "table: {text}");
}

#[test]
fn untracked_module_missing_reason_renders_in_table() {
    let eval = SeriesEvaluation {
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(
            pin_for("ra"),
            Verdict::Incompatible {
                reasons: vec![Reason::UntrackedModuleMissing {
                    module: ModuleName::new("rabbit_mgmt_wm_user").unwrap(),
                }],
            },
        )]),
        diagnostics: Diagnostics::default(),
        patch_facts: Default::default(),
    };
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("UntrackedModuleMissing"), "table: {text}");
    assert!(text.contains("rabbit_mgmt_wm_user"), "table: {text}");
}

#[test]
fn unsupported_file_type_reason_renders_in_table() {
    let eval = SeriesEvaluation {
        verdict: SeriesVerdict::from_results(vec![PinVerdict::new(
            pin_for("demo"),
            Verdict::RequiresAdaptation {
                reasons: vec![Reason::UnsupportedFileType {
                    path: PathBuf::from("lib/foo.ex"),
                }],
            },
        )]),
        diagnostics: Diagnostics::default(),
        patch_facts: Default::default(),
    };
    let text = render_evaluation_table(&eval, TableStyle::Modern);
    assert!(text.contains("UnsupportedFileType"), "table: {text}");
    assert!(text.contains("lib/foo.ex"), "table: {text}");
}

#[test]
fn ascii_style_avoids_box_drawing_characters() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = EvaluationContext::new(pin_for("ra"), snap, scope);
    let diff = "\
diff --git a/x.erl b/x.erl
--- a/x.erl
+++ b/x.erl
@@ -1,1 +1,2 @@
 -module(x).
+go() -> ra:start().
";
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[context]);
    let ascii = render_evaluation_table(&eval, TableStyle::Ascii);
    for ch in ascii.chars() {
        assert!(
            ch.is_ascii(),
            "ascii table should be 7-bit clean, got `{ch}`",
        );
    }
}
