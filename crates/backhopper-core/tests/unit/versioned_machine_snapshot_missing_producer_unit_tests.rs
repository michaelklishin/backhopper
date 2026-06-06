// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::config::{FamilyDefaults, VersionedMachineImplDecl, WireConstantDecl};
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, MacroName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    Module, Provenance, Snapshot, SnapshotHeader, VersionedMachineVersion, WireConstantBinding,
    WireValue, state,
};
use backhopper_core::model::verdict::{Reason, SnapshotSide};
use time::OffsetDateTime;

const RABBIT_FIFO_TOUCH: &str = "\
diff --git a/deps/rabbit/src/rabbit_fifo.erl b/deps/rabbit/src/rabbit_fifo.erl
--- a/deps/rabbit/src/rabbit_fifo.erl
+++ b/deps/rabbit/src/rabbit_fifo.erl
@@ -1,1 +1,2 @@
 -module(rabbit_fifo).
+%% touched
";

const RA_LOG_SEGMENT_TOUCH: &str = "\
diff --git a/deps/ra/src/ra_log_segment.erl b/deps/ra/src/ra_log_segment.erl
--- a/deps/ra/src/ra_log_segment.erl
+++ b/deps/ra/src/ra_log_segment.erl
@@ -1,1 +1,2 @@
 -module(ra_log_segment).
+%% touched
";

const UNRELATED_TOUCH: &str = "\
diff --git a/deps/rabbit/src/unrelated.erl b/deps/rabbit/src/unrelated.erl
--- a/deps/rabbit/src/unrelated.erl
+++ b/deps/rabbit/src/unrelated.erl
@@ -1,1 +1,2 @@
 -module(unrelated).
+%% touched
";

fn header(tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new("rabbitmq-server").unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: Vec::new(),
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

fn rabbit_fifo_with_version(value: u64) -> Module {
    let mut m = Module::new(ModuleName::new("rabbit_fifo").unwrap());
    m.versioned_machine_version = Some(VersionedMachineVersion {
        function: FunctionName::new("version").unwrap(),
        arity: Arity::new(0),
        value: Some(value),
        provenance: Provenance::Literal,
    });
    m
}

fn rabbit_fifo_without_version() -> Module {
    Module::new(ModuleName::new("rabbit_fifo").unwrap())
}

fn ra_log_segment_with(macros: &[&str]) -> Module {
    let mut m = Module::new(ModuleName::new("ra_log_segment").unwrap());
    m.wire_constants = macros
        .iter()
        .map(|name| WireConstantBinding {
            macro_name: MacroName::new(*name).unwrap(),
            value: WireValue::U64(1),
            defined_in: None,
        })
        .collect();
    m
}

fn canonical(tag: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header(tag), modules, Vec::new()).into_canonical()
}

fn rabbitmq_defaults() -> FamilyDefaults {
    FamilyDefaults {
        wire_constants: Vec::new(),
        versioned_machines: Vec::new(),
        versioned_machine_impls: vec![VersionedMachineImplDecl {
            module: "rabbit_fifo".into(),
            version_function: "version".into(),
            allow_state_flag_gating: true,
        }],
        test_helper_search_paths: Vec::new(),
    }
}

fn ra_defaults() -> FamilyDefaults {
    FamilyDefaults {
        wire_constants: vec![WireConstantDecl::new(
            "ra_log_segment",
            &["VERSION", "MAGIC"],
        )],
        versioned_machines: Vec::new(),
        versioned_machine_impls: Vec::new(),
        test_helper_search_paths: Vec::new(),
    }
}

fn evaluate_with(
    diff: &str,
    target: Snapshot<state::Canonical>,
    source: Snapshot<state::Canonical>,
    defaults: FamilyDefaults,
) -> Vec<Reason> {
    let pin = Pin::new(
        ProjectName::new("rabbitmq-server").unwrap(),
        TagName::new("target").unwrap(),
    );
    let scope = PinScope::from_snapshot(
        ProjectName::new("rabbitmq-server").unwrap(),
        &target,
        Vec::new(),
    );
    let ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_source_snapshot(source)
        .with_family_defaults(defaults);
    let series = Patch::parse(diff.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    series.verdict.results[0].verdict.reasons().to_vec()
}

#[test]
fn versioned_machine_missing_on_both_sides_fires_with_side_both() {
    let target = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let source = canonical("v4.0.x", vec![rabbit_fifo_without_version()]);
    let reasons = evaluate_with(RABBIT_FIFO_TOUCH, target, source, rabbitmq_defaults());
    assert!(reasons.iter().any(|r| matches!(
        r,
        Reason::VersionedMachineSnapshotMissing { module, side }
            if module.as_str() == "rabbit_fifo" && *side == SnapshotSide::Both
    )));
}

#[test]
fn versioned_machine_missing_on_target_only_fires_with_side_target() {
    let target = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let source = canonical("v4.0.x", vec![rabbit_fifo_with_version(7)]);
    let reasons = evaluate_with(RABBIT_FIFO_TOUCH, target, source, rabbitmq_defaults());
    assert!(reasons.iter().any(|r| matches!(
        r,
        Reason::VersionedMachineSnapshotMissing { module, side }
            if module.as_str() == "rabbit_fifo" && *side == SnapshotSide::Target
    )));
}

#[test]
fn versioned_machine_missing_on_source_only_fires_with_side_source() {
    let target = canonical("v4.0.x", vec![rabbit_fifo_with_version(7)]);
    let source = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let reasons = evaluate_with(RABBIT_FIFO_TOUCH, target, source, rabbitmq_defaults());
    assert!(reasons.iter().any(|r| matches!(
        r,
        Reason::VersionedMachineSnapshotMissing { module, side }
            if module.as_str() == "rabbit_fifo" && *side == SnapshotSide::Source
    )));
}

#[test]
fn versioned_machine_present_on_both_sides_does_not_fire_missing() {
    let target = canonical("v4.0.x", vec![rabbit_fifo_with_version(7)]);
    let source = canonical("v4.1.x", vec![rabbit_fifo_with_version(8)]);
    let reasons = evaluate_with(RABBIT_FIFO_TOUCH, target, source, rabbitmq_defaults());
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::VersionedMachineSnapshotMissing { .. }))
    );
}

#[test]
fn untouched_declared_module_does_not_fire() {
    let target = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let source = canonical("v4.0.x", vec![rabbit_fifo_without_version()]);
    let reasons = evaluate_with(UNRELATED_TOUCH, target, source, rabbitmq_defaults());
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, Reason::VersionedMachineSnapshotMissing { .. }))
    );
}

#[test]
fn wire_constant_bindings_missing_lists_only_absent_macros_sorted() {
    let target = canonical("v4.0.x", vec![ra_log_segment_with(&["VERSION"])]);
    let source = canonical("v4.0.x", vec![ra_log_segment_with(&["VERSION", "MAGIC"])]);
    let reasons = evaluate_with(RA_LOG_SEGMENT_TOUCH, target, source, ra_defaults());
    let row = reasons.iter().find_map(|r| match r {
        Reason::WireConstantBindingsMissing {
            module,
            macros,
            side,
        } if module.as_str() == "ra_log_segment" => Some((macros.clone(), *side)),
        _ => None,
    });
    let (macros, side) = row.expect("expected one WireConstantBindingsMissing reason");
    assert_eq!(side, SnapshotSide::Target);
    let names: Vec<&str> = macros.iter().map(MacroName::as_str).collect();
    assert_eq!(names, vec!["MAGIC"]);
}

#[test]
fn wire_constant_bindings_missing_on_both_collapses_to_side_both() {
    let target = canonical("v4.0.x", vec![ra_log_segment_with(&[])]);
    let source = canonical("v4.0.x", vec![ra_log_segment_with(&[])]);
    let reasons = evaluate_with(RA_LOG_SEGMENT_TOUCH, target, source, ra_defaults());
    let row = reasons.iter().find_map(|r| match r {
        Reason::WireConstantBindingsMissing {
            module,
            macros,
            side,
        } if module.as_str() == "ra_log_segment" => Some((macros.clone(), *side)),
        _ => None,
    });
    let (macros, side) = row.expect("expected one WireConstantBindingsMissing reason");
    assert_eq!(side, SnapshotSide::Both);
    let names: Vec<&str> = macros.iter().map(MacroName::as_str).collect();
    assert_eq!(names, vec!["MAGIC", "VERSION"]);
}

#[test]
fn divergent_missing_sets_emit_two_reasons_one_per_side() {
    let target = canonical("v4.0.x", vec![ra_log_segment_with(&["VERSION"])]);
    let source = canonical("v4.0.x", vec![ra_log_segment_with(&["MAGIC"])]);
    let reasons = evaluate_with(RA_LOG_SEGMENT_TOUCH, target, source, ra_defaults());
    let mut by_side: Vec<(SnapshotSide, Vec<String>)> = reasons
        .iter()
        .filter_map(|r| match r {
            Reason::WireConstantBindingsMissing {
                module,
                macros,
                side,
            } if module.as_str() == "ra_log_segment" => Some((
                *side,
                macros.iter().map(|m| m.as_str().to_owned()).collect(),
            )),
            _ => None,
        })
        .collect();
    by_side.sort_by_key(|(s, _)| match s {
        SnapshotSide::Source => 0,
        SnapshotSide::Target => 1,
        SnapshotSide::Both => 2,
    });
    assert_eq!(by_side.len(), 2);
    assert_eq!(by_side[0].0, SnapshotSide::Source);
    assert_eq!(by_side[0].1, vec!["VERSION"]);
    assert_eq!(by_side[1].0, SnapshotSide::Target);
    assert_eq!(by_side[1].1, vec!["MAGIC"]);
}

#[test]
fn versioned_machine_missing_target_only_fires_when_no_source_snapshot() {
    let pin = Pin::new(
        ProjectName::new("rabbitmq-server").unwrap(),
        TagName::new("target").unwrap(),
    );
    let target = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let scope = PinScope::from_snapshot(
        ProjectName::new("rabbitmq-server").unwrap(),
        &target,
        Vec::new(),
    );
    let ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_family_defaults(rabbitmq_defaults());
    let series = Patch::parse(RABBIT_FIFO_TOUCH.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons: Vec<Reason> = series.verdict.results[0].verdict.reasons().to_vec();
    assert!(reasons.iter().any(|r| matches!(
        r,
        Reason::VersionedMachineSnapshotMissing { module, side }
            if module.as_str() == "rabbit_fifo" && *side == SnapshotSide::Target
    )));
}

#[test]
fn wire_constants_missing_on_target_fires_when_no_source_snapshot() {
    let pin = Pin::new(
        ProjectName::new("rabbitmq-server").unwrap(),
        TagName::new("target").unwrap(),
    );
    let target = canonical("v4.0.x", vec![ra_log_segment_with(&["VERSION"])]);
    let scope = PinScope::from_snapshot(
        ProjectName::new("rabbitmq-server").unwrap(),
        &target,
        Vec::new(),
    );
    let ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new())
        .with_family_defaults(ra_defaults());
    let series = Patch::parse(RA_LOG_SEGMENT_TOUCH.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons: Vec<Reason> = series.verdict.results[0].verdict.reasons().to_vec();
    let row = reasons.iter().find_map(|r| match r {
        Reason::WireConstantBindingsMissing {
            module,
            macros,
            side,
        } if module.as_str() == "ra_log_segment" => Some((macros.clone(), *side)),
        _ => None,
    });
    let (macros, side) = row.expect("expected one missing reason");
    assert_eq!(side, SnapshotSide::Target);
    let names: Vec<&str> = macros.iter().map(MacroName::as_str).collect();
    assert_eq!(names, vec!["MAGIC"]);
}

#[test]
fn omitting_family_defaults_skips_the_new_checks() {
    let pin = Pin::new(
        ProjectName::new("rabbitmq-server").unwrap(),
        TagName::new("target").unwrap(),
    );
    let target = canonical("v3.13.x", vec![rabbit_fifo_without_version()]);
    let scope = PinScope::from_snapshot(
        ProjectName::new("rabbitmq-server").unwrap(),
        &target,
        Vec::new(),
    );
    let ctx = EvaluationContext::for_pin(pin, target)
        .with_scope(scope)
        .with_files(EvaluationFiles::new());
    let series = Patch::parse(RABBIT_FIFO_TOUCH.as_bytes())
        .unwrap()
        .analyze()
        .evaluate_series(&[ctx]);
    let reasons: Vec<Reason> = series.verdict.results[0].verdict.reasons().to_vec();
    assert!(!reasons.iter().any(|r| matches!(
        r,
        Reason::VersionedMachineSnapshotMissing { .. } | Reason::WireConstantBindingsMissing { .. }
    )));
}
