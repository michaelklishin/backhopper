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
        project: ProjectName::new("rabbit").unwrap(),
        tag: TagName::new("v4.0.0").unwrap(),
        branch: None,
        commit: CommitSha::from_str("0000000000000000000000000000000000000000").unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    };
    Snapshot::from_extracted(
        header,
        vec![Module::new(ModuleName::new("rabbit_db_queue").unwrap())],
        Vec::new(),
    )
    .into_canonical()
}

fn suggested(diff: &str) -> Vec<String> {
    let patch = Patch::parse(diff.as_bytes()).unwrap().analyze();
    let s = snap();
    let scope = PinScope::from_snapshot(ProjectName::new("rabbit").unwrap(), &s, Vec::new());
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("rabbit").unwrap(),
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
-{mapping, \"management.tcp.port\", \"rabbitmq_management.tcp_config.port\", []}.
+{mapping, \"management.tcp.port\", \"rabbitmq_management.listener.port\", []}.
";

const NON_SCHEMA_DIFF: &str = "\
diff --git a/src/rabbit_db_queue.erl b/src/rabbit_db_queue.erl
--- a/src/rabbit_db_queue.erl
+++ b/src/rabbit_db_queue.erl
@@ -1,1 +1,2 @@
 -module(rabbit_db_queue).
+is_empty() -> ok.
";

const SNIPPETS_DIFF: &str = "\
diff --git a/deps/rabbit/priv/schema/advanced.snippets b/deps/rabbit/priv/schema/advanced.snippets
--- a/deps/rabbit/priv/schema/advanced.snippets
+++ b/deps/rabbit/priv/schema/advanced.snippets
@@ -1,1 +1,2 @@
 listeners.tcp.default = 5672
+vm_memory_high_watermark.relative = 0.6
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
    let scope = PinScope::from_snapshot(ProjectName::new("rabbit").unwrap(), &s, Vec::new());
    let mut files = EvaluationFiles::new();
    files = files.with(
        PathBuf::from("deps/rabbitmq_management/priv/schema/rabbitmq_management.schema"),
        Some(
            b"{mapping, \"management.tcp.port\", \"rabbitmq_management.tcp_config.port\", []}.\n"
                .to_vec(),
        ),
    );
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("rabbit").unwrap(),
            TagName::new("target").unwrap(),
        ),
        s,
    )
    .with_scope(scope)
    .with_files(files);
    let series = patch.evaluate_series(&[ctx]);
    assert!(!series.diagnostics.suggested_suites.is_empty());
}
