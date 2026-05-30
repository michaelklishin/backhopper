// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;
use std::str::FromStr;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Module, Snapshot, SnapshotHeader, state};
use time::OffsetDateTime;

fn snap() -> Snapshot<state::Canonical> {
    let header = SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new("v").unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
    };
    Snapshot::from_extracted(
        header,
        vec![Module::new(ModuleName::new("u").unwrap())],
        Vec::new(),
    )
    .into_canonical()
}

fn suggested(diff: &str) -> Vec<String> {
    let patch = Patch::parse(diff.as_bytes()).unwrap().analyze();
    let s = snap();
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &s, Vec::new());
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        s,
    )
    .with_scope(scope)
    .with_files(EvaluationFiles::new());
    patch.evaluate_series(&[ctx]).diagnostics.suggested_suites
}

const SCHEMA_DIFF: &str = "\
diff --git a/deps/rabbitmq_management/priv/schema/rabbitmq_management.schema b/deps/rabbitmq_management/priv/schema/rabbitmq_management.schema
--- a/deps/rabbitmq_management/priv/schema/rabbitmq_management.schema
+++ b/deps/rabbitmq_management/priv/schema/rabbitmq_management.schema
@@ -1,1 +1,1 @@
-{mapping, \"x\", \"y\", []}.
+{mapping, \"x\", \"z\", []}.
";

const NON_SCHEMA_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,1 +1,2 @@
 -module(foo).
+do() -> ok.
";

const SNIPPETS_DIFF: &str = "\
diff --git a/deps/rabbit/priv/schema/foo.snippets b/deps/rabbit/priv/schema/foo.snippets
--- a/deps/rabbit/priv/schema/foo.snippets
+++ b/deps/rabbit/priv/schema/foo.snippets
@@ -1,1 +1,2 @@
 a.b = c
+x.y = z
";

#[test]
fn schema_diff_suggests_plugin_config_schema_suite() {
    let suites = suggested(SCHEMA_DIFF);
    assert!(
        suites.contains(&"rabbitmq_management_config_schema_SUITE".to_owned()),
        "expected rabbitmq_management_config_schema_SUITE, got {suites:?}"
    );
}

#[test]
fn snippets_diff_suggests_plugin_config_schema_suite() {
    let suites = suggested(SNIPPETS_DIFF);
    assert!(
        suites.contains(&"rabbit_config_schema_SUITE".to_owned()),
        "expected rabbit_config_schema_SUITE, got {suites:?}"
    );
}

#[test]
fn non_schema_diff_yields_no_suite_suggestions() {
    let suites = suggested(NON_SCHEMA_DIFF);
    assert!(suites.is_empty(), "got {suites:?}");
}

#[test]
fn schema_diff_suggestions_survive_inapplicable_promotion() {
    // schema-only diff promotes to Inapplicable::OnlySchemaTouched.
    // The suggested suites must still be populated.
    let patch = Patch::parse(SCHEMA_DIFF.as_bytes()).unwrap().analyze();
    let s = snap();
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &s, Vec::new());
    let mut files = EvaluationFiles::new();
    files = files.with(
        PathBuf::from("deps/rabbitmq_management/priv/schema/rabbitmq_management.schema"),
        Some(b"{mapping, \"x\", \"y\", []}.\n".to_vec()),
    );
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        s,
    )
    .with_scope(scope)
    .with_files(files);
    let series = patch.evaluate_series(&[ctx]);
    assert!(!series.diagnostics.suggested_suites.is_empty());
}
