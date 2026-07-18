// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::verdict::Reason;
use backhopper_test_support::{canonical_snapshot, module_with, snapshot_header};

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("demo").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

fn snapshot() -> Snapshot<state::Canonical> {
    canonical_snapshot(
        snapshot_header("demo", "v1.0.0"),
        vec![module_with("demo", &[("greet", 1)])],
    )
}

fn run(patch: &str, pin_file: &[u8]) -> Vec<Reason> {
    let files = EvaluationFiles::new().with(PathBuf::from("src/demo.erl"), Some(pin_file.to_vec()));
    let snapshot = snapshot();
    let scope = PinScope::from_snapshot(pin().project.clone(), &snapshot, Vec::new());
    let ctx = EvaluationContext::for_pin(pin(), snapshot)
        .with_scope(scope)
        .with_files(files);
    let series = Patch::parse(patch.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    series.verdict.results[0].verdict.reasons().to_vec()
}

const PATCH: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,3 +1,4 @@
 -module(demo).
 -export([greet/1]).
+example() -> demo:greet(<<\"x\">>).
 greet(Name) -> Name.
";

#[test]
fn exact_match_emits_no_preimage_reason() {
    let target = b"-module(demo).\n-export([greet/1]).\ngreet(Name) -> Name.\n";
    let reasons = run(PATCH, target);
    assert!(
        reasons.iter().all(|r| !matches!(
            r,
            Reason::PreimageDrifted { .. } | Reason::PreimageMissing { .. }
        )),
        "expected no preimage reasons, got {reasons:?}"
    );
}

#[test]
fn drifted_offset_emits_preimage_drifted_with_positive_delta() {
    let target = b"%% preamble line one\n%% preamble line two\n-module(demo).\n-export([greet/1]).\ngreet(Name) -> Name.\n";
    let reasons = run(PATCH, target);
    let drift = reasons
        .iter()
        .find_map(|r| match r {
            Reason::PreimageDrifted { line_delta, .. } => Some(*line_delta),
            _ => None,
        })
        .expect("expected PreimageDrifted reason");
    assert_eq!(drift, 2, "preimage moved two lines down");
}

#[test]
fn missing_preimage_emits_preimage_missing_with_excerpt() {
    let target = b"-module(other).\nother() -> ok.\n";
    let reasons = run(PATCH, target);
    let excerpt = reasons
        .iter()
        .find_map(|r| match r {
            Reason::PreimageMissing {
                preimage_excerpt, ..
            } => Some(preimage_excerpt.clone()),
            _ => None,
        })
        .expect("expected PreimageMissing reason");
    assert!(
        excerpt.contains("-module(demo)"),
        "excerpt should start with the preimage; got {excerpt:?}"
    );
}

#[test]
fn no_preimage_reason_when_preimage_block_is_empty() {
    // A pure-insertion hunk has nothing to verify against the pin's file.
    let patch = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,0 +1,1 @@
+new_only() -> ok.
";
    let target = b"-module(demo).\n-export([greet/1]).\ngreet(Name) -> Name.\n";
    let reasons = run(patch, target);
    assert!(
        reasons.iter().all(|r| !matches!(
            r,
            Reason::PreimageDrifted { .. } | Reason::PreimageMissing { .. }
        )),
        "pure insertion has no preimage to verify; got {reasons:?}"
    );
}

#[test]
fn preimage_missing_is_non_blocking() {
    let target = b"completely different\nfile content\n";
    let reasons = run(PATCH, target);
    let blocking = reasons.iter().any(Reason::is_blocking);
    let has_missing = reasons
        .iter()
        .any(|r| matches!(r, Reason::PreimageMissing { .. }));
    assert!(has_missing, "expected PreimageMissing; got {reasons:?}");
    assert!(
        !blocking,
        "PreimageMissing alone must not be blocking; got {reasons:?}"
    );
}

#[test]
fn preimage_drifted_is_non_blocking() {
    let target = b"%% header line\n-module(demo).\n-export([greet/1]).\ngreet(Name) -> Name.\n";
    let reasons = run(PATCH, target);
    let blocking = reasons.iter().any(Reason::is_blocking);
    let has_drift = reasons
        .iter()
        .any(|r| matches!(r, Reason::PreimageDrifted { .. }));
    assert!(has_drift, "expected PreimageDrifted; got {reasons:?}");
    assert!(
        !blocking,
        "PreimageDrifted alone must not be blocking; got {reasons:?}"
    );
}
