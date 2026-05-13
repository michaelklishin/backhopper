use std::path::PathBuf;

use backhopper_core::compat::patch::{Language, Patch, PinContext};
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

use backhopper_cli::tables::render_evaluation_table;

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src/**/*.erl".into()],
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
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
    let context = PinContext::new(pin_for("ra"), snap, scope);
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
        .analyze(Language::Erlang)
        .evaluate_series(&[context]);
    let text = render_evaluation_table(&eval);
    assert!(text.contains("ra@v1.0.0"), "table: {}", text);
    assert!(text.contains("Compatible"), "table: {}", text);
    assert!(
        text.contains("1 tracked symbol referenced"),
        "table: {}",
        text
    );
}

#[test]
fn table_lists_missing_symbol_reason_with_mfa_detail() {
    let snap = snapshot_with_module("ra", "ra", &[("start", 0)]);
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let context = PinContext::new(pin_for("ra"), snap, scope);
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
        .analyze(Language::Erlang)
        .evaluate_series(&[context]);
    let text = render_evaluation_table(&eval);
    assert!(text.contains("Incompatible"), "table: {}", text);
    assert!(text.contains("MissingSymbol"), "table: {}", text);
    assert!(text.contains("ra:gone/3"), "table: {}", text);
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
    };
    let text = render_evaluation_table(&eval);
    assert!(text.contains("RecordFields"), "table: {}", text);
    assert!(text.contains("FileAbsent"), "table: {}", text);
    assert!(text.contains("src/a.erl"), "table: {}", text);
}
