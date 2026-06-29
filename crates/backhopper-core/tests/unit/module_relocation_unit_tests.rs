// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Module, Snapshot, state};
use backhopper_core::model::verdict::{Reason, Verdict};
use backhopper_test_support::{canonical_snapshot, module, snapshot_header};

fn snap_with(modules: Vec<&str>) -> Snapshot<state::Canonical> {
    let mods: Vec<Module> = modules.into_iter().map(module).collect();
    canonical_snapshot(snapshot_header("p", "v"), mods)
}

fn evaluate(diff: &str, modules_on_pin: Vec<&str>, file_on_disk: bool) -> Vec<Reason> {
    let snapshot = snap_with(modules_on_pin);
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &snapshot, Vec::new());
    let mut files = EvaluationFiles::new();
    let touched_path = PathBuf::from("deps/rabbit/src/rabbit_amqp_reader.erl");
    if file_on_disk {
        files = files.with(
            touched_path.clone(),
            Some(b"-module(rabbit_amqp_reader).\n".to_vec()),
        );
    } else {
        files = files.with(touched_path, None);
    }
    let patch = Patch::parse(diff.as_bytes()).unwrap().analyze();
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        snapshot,
    )
    .with_scope(scope)
    .with_files(files);
    let series = patch.evaluate_series(&[ctx]);
    match &series.verdict.results[0].verdict {
        Verdict::Compatible | Verdict::Inapplicable { .. } => Vec::new(),
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            reasons.clone()
        }
    }
}

const TOUCH_DIFF: &str = "\
diff --git a/deps/rabbit/src/rabbit_amqp_reader.erl b/deps/rabbit/src/rabbit_amqp_reader.erl
--- a/deps/rabbit/src/rabbit_amqp_reader.erl
+++ b/deps/rabbit/src/rabbit_amqp_reader.erl
@@ -1,1 +1,2 @@
 -module(rabbit_amqp_reader).
+harden() -> ok.
";

#[test]
fn module_relocated_fires_when_basename_module_exists_elsewhere() {
    let reasons = evaluate(TOUCH_DIFF, vec!["rabbit_amqp_reader"], false);
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            Reason::ModuleRelocated { module, .. } if module.as_str() == "rabbit_amqp_reader"
        )),
        "expected ModuleRelocated, got {reasons:?}"
    );
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::FileAbsent { .. })),
        "ModuleRelocated should take precedence over FileAbsent, got {reasons:?}"
    );
}

#[test]
fn file_absent_fires_when_module_truly_missing() {
    let reasons = evaluate(TOUCH_DIFF, vec!["unrelated_module"], false);
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, Reason::FileAbsent { .. })),
        "expected FileAbsent for truly missing module, got {reasons:?}"
    );
}

#[test]
fn neither_fires_when_file_present_on_disk() {
    let reasons = evaluate(TOUCH_DIFF, vec!["rabbit_amqp_reader"], true);
    assert!(
        !reasons.iter().any(|r| matches!(
            r,
            Reason::ModuleRelocated { .. } | Reason::FileAbsent { .. }
        )),
        "neither should fire when the file IS on disk, got {reasons:?}"
    );
}

#[test]
fn module_relocated_does_not_make_verdict_incompatible() {
    // RequiresAdaptation, not Incompatible
    let snapshot = snap_with(vec!["rabbit_amqp_reader"]);
    let scope = PinScope::from_snapshot(ProjectName::new("p").unwrap(), &snapshot, Vec::new());
    let mut files = EvaluationFiles::new();
    files = files.with(
        PathBuf::from("deps/rabbit/src/rabbit_amqp_reader.erl"),
        None,
    );
    let patch = Patch::parse(TOUCH_DIFF.as_bytes()).unwrap().analyze();
    let ctx = EvaluationContext::for_pin(
        Pin::new(
            ProjectName::new("p").unwrap(),
            TagName::new("target").unwrap(),
        ),
        snapshot,
    )
    .with_scope(scope)
    .with_files(files);
    let series = patch.evaluate_series(&[ctx]);
    assert!(
        matches!(
            series.verdict.results[0].verdict,
            Verdict::RequiresAdaptation { .. }
        ),
        "ModuleRelocated alone should not be blocking; got {:?}",
        series.verdict.results[0].verdict
    );
}
