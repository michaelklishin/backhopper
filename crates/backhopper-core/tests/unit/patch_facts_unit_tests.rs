// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName, RecordName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Module, Snapshot, SnapshotHeader, state};
use backhopper_core::model::verdict::LoggingStyle;
use time::OffsetDateTime;

fn snap(modules: Vec<Module>) -> Snapshot<state::Canonical> {
    let header = SnapshotHeader {
        project: ProjectName::new("p").unwrap(),
        tag: TagName::new("v").unwrap(),
        branch: None,
        commit: CommitSha::from_str_anonymous(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
    };
    Snapshot::from_extracted(header, modules, Vec::new()).into_canonical()
}

trait CommitShaTestExt {
    fn from_str_anonymous() -> Self;
}

impl CommitShaTestExt for CommitSha {
    fn from_str_anonymous() -> Self {
        use std::str::FromStr;
        CommitSha::from_str("0000000000000000000000000000000000000000").unwrap()
    }
}

fn facts_for(diff: &str) -> backhopper_core::model::verdict::PatchFacts {
    let patch = Patch::parse(diff.as_bytes()).unwrap().analyze();
    let pin_snap = snap(vec![Module::new(ModuleName::new("_unused").unwrap())]);
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &pin_snap, Vec::new());
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        pin_snap,
    )
    .with_scope(scope)
    .with_files(EvaluationFiles::new());
    patch.evaluate_series(&[ctx]).patch_facts
}

const RABBIT_LOG_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,1 +1,3 @@
 -module(foo).
+do() -> rabbit_log:info(\"hi\", []).
+other() -> rabbit_log:warning(\"bye\", []).
";

const LOGGER_MACRO_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,1 +1,3 @@
 -module(foo).
+do() -> ?LOG_INFO(\"hi\", []).
+other() -> ?LOG_WARNING(\"bye\", []).
";

const MIXED_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,1 +1,3 @@
 -module(foo).
+do() -> ?LOG_INFO(\"hi\", []).
+other() -> rabbit_log:warning(\"bye\", []).
";

const KHEPRI_ONLY_DIFF: &str = "\
diff --git a/src/rabbit_khepri.erl b/src/rabbit_khepri.erl
--- a/src/rabbit_khepri.erl
+++ b/src/rabbit_khepri.erl
@@ -1,1 +1,2 @@
 -module(rabbit_khepri).
+do_in_khepri() -> ok.
";

const DUAL_BRANCH_DIFF: &str = "\
diff --git a/src/rabbit_db_binding.erl b/src/rabbit_db_binding.erl
--- a/src/rabbit_db_binding.erl
+++ b/src/rabbit_db_binding.erl
@@ -1,1 +1,3 @@
 -module(rabbit_db_binding).
+do_in_khepri(X) -> X.
+do_in_mnesia(X) -> X.
";

const VERSIONED_RECORD_DIFF: &str = "\
diff --git a/include/foo.hrl b/include/foo.hrl
--- a/include/foo.hrl
+++ b/include/foo.hrl
@@ -1,0 +1,1 @@
+-record(topic_trie_edge_v2, {trie_edge, node_id, child_count}).
";

const KHEPRI_MACRO_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,1 +1,2 @@
 -module(foo).
+handle() -> ?khepri_error(name, #{}).
";

#[test]
fn logging_style_pure_rabbit_log_module() {
    let facts = facts_for(RABBIT_LOG_DIFF);
    let entry = facts
        .logging_style
        .get(&PathBuf::from("src/foo.erl"))
        .expect("logging style should be classified");
    assert_eq!(entry.dominant, LoggingStyle::RabbitLogModule);
    assert_eq!(entry.rabbit_log_sites, 2);
    assert_eq!(entry.logger_macro_sites, 0);
}

#[test]
fn logging_style_pure_logger_macros() {
    let facts = facts_for(LOGGER_MACRO_DIFF);
    let entry = facts
        .logging_style
        .get(&PathBuf::from("src/foo.erl"))
        .unwrap();
    assert_eq!(entry.dominant, LoggingStyle::LoggerMacros);
    assert_eq!(entry.logger_macro_sites, 2);
    assert_eq!(entry.rabbit_log_sites, 0);
}

#[test]
fn logging_style_mixed_detected() {
    let facts = facts_for(MIXED_DIFF);
    let entry = facts
        .logging_style
        .get(&PathBuf::from("src/foo.erl"))
        .unwrap();
    assert_eq!(entry.dominant, LoggingStyle::Mixed);
    assert_eq!(entry.logger_macro_sites, 1);
    assert_eq!(entry.rabbit_log_sites, 1);
}

#[test]
fn khepri_signals_pure_khepri_only_branch() {
    let facts = facts_for(KHEPRI_ONLY_DIFF);
    let k = &facts.khepri_signals;
    assert!(k.touches_khepri_module);
    assert!(k.touches_only_khepri_branch);
    assert!(!k.touches_dual_branch);
}

#[test]
fn khepri_signals_dual_branch_overrides_only() {
    let facts = facts_for(DUAL_BRANCH_DIFF);
    let k = &facts.khepri_signals;
    assert!(k.touches_dual_branch);
    assert!(!k.touches_only_khepri_branch);
}

#[test]
fn khepri_macros_grep_detected() {
    let facts = facts_for(KHEPRI_MACRO_DIFF);
    assert!(facts.khepri_signals.uses_khepri_macros);
}

#[test]
fn versioned_record_detection_picks_up_v2_suffix() {
    let facts = facts_for(VERSIONED_RECORD_DIFF);
    let expected = RecordName::new("topic_trie_edge_v2").unwrap();
    assert!(
        facts.introduces_versioned_record.contains(&expected),
        "expected topic_trie_edge_v2 in versioned records, got {:?}",
        facts.introduces_versioned_record
    );
}

#[test]
fn versioned_record_ignores_unversioned_decls() {
    let diff = "\
diff --git a/include/foo.hrl b/include/foo.hrl
--- a/include/foo.hrl
+++ b/include/foo.hrl
@@ -1,0 +1,1 @@
+-record(plain, {a, b, c}).
";
    let facts = facts_for(diff);
    assert!(facts.introduces_versioned_record.is_empty());
}
