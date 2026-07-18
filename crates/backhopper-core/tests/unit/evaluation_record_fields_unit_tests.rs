// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `RecordFieldsChanged`: a source record with extra fields fires,
//! whether the record is declared in an `.hrl` or inline in a module.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{FieldName, ModuleName, ProjectName, RecordName};
use backhopper_core::model::snapshot::{HrlFile, Module, RecordDecl, RecordField, Snapshot, state};
use backhopper_core::model::verdict::Reason;
use backhopper_test_support::{canonical_snapshot, pin, snapshot_header};

fn snapshot_with_record_fields(
    project: &str,
    record_name: &str,
    fields: Vec<&str>,
) -> Snapshot<state::Canonical> {
    let mut hrl = HrlFile::new(format!("include/{project}.hrl"));
    hrl.records.push(RecordDecl {
        name: RecordName::new(record_name).unwrap(),
        fields: fields
            .into_iter()
            .map(|n| RecordField {
                name: FieldName::new(n).unwrap(),
                type_repr: None,
            })
            .collect(),
    });
    let m = Module::new(ModuleName::new("demo").unwrap());
    Snapshot::from_extracted(snapshot_header(project, "v1.0.0"), vec![m], vec![hrl])
        .into_canonical()
}

fn snapshot_with_inline_record_fields(
    project: &str,
    record_name: &str,
    fields: Vec<&str>,
) -> Snapshot<state::Canonical> {
    let mut m = Module::new(ModuleName::new("demo").unwrap());
    m.records.push(RecordDecl {
        name: RecordName::new(record_name).unwrap(),
        fields: fields
            .into_iter()
            .map(|n| RecordField {
                name: FieldName::new(n).unwrap(),
                type_repr: None,
            })
            .collect(),
    });
    canonical_snapshot(snapshot_header(project, "v1.0.0"), vec![m])
}

#[test]
fn record_fields_changed_fires_when_source_record_has_extra_fields() {
    let target = snapshot_with_record_fields("demo", "user", vec!["id", "name"]);
    let source = snapshot_with_record_fields("demo", "user", vec!["id", "name", "email"]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+email_of(U) -> U#user.email.
";
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin("demo", "v1.0.0"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_record_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::RecordFieldsChanged { record, expected, found }
                if record.as_str() == "user"
                    && expected.len() == 3
                    && found.len() == 2
        )
    });
    assert!(
        has_record_changed,
        "expected RecordFieldsChanged reason, got {reasons:?}"
    );
}

// Records declared inline in a module must resolve, not only those in headers.
#[test]
fn record_fields_changed_fires_for_inline_module_record() {
    let target = snapshot_with_inline_record_fields("demo", "user", vec!["id", "name"]);
    let source = snapshot_with_inline_record_fields("demo", "user", vec!["id", "name", "email"]);
    let diff = "\
diff --git a/rabbit_fifo.erl b/rabbit_fifo.erl
--- a/rabbit_fifo.erl
+++ b/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+email_of(U) -> U#user.email.
";
    let scope = PinScope::from_snapshot(ProjectName::new("demo").unwrap(), &target, []);
    let ctx = EvaluationContext::new(pin("demo", "v1.0.0"), target, scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source);
    let eval = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons = eval.verdict.results[0].verdict.reasons();
    let has_record_changed = reasons.iter().any(|r| {
        matches!(
            r,
            Reason::RecordFieldsChanged { record, expected, found }
                if record.as_str() == "user"
                    && expected.len() == 3
                    && found.len() == 2
        )
    });
    assert!(
        has_record_changed,
        "expected RecordFieldsChanged reason, got {reasons:?}"
    );
}
