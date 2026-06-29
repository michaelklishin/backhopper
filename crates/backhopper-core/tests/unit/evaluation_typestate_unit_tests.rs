// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Type-state checks for the `EvaluationContext<S>` pipeline. These
//! tests assert at the value level what the type system already enforces
//! at compile time: `Pinned` → `Scoped` → `Sourced` is the only path,
//! and only `Scoped` and `Sourced` implement `EvaluationInput`.

use std::path::PathBuf;

use backhopper_core::compat::patch::{
    EvaluationContext, EvaluationFiles, EvaluationInput, Pinned, Scoped, Sourced,
};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_test_support::{canonical_snapshot, module_with, snapshot_header};

fn snap(project: &str) -> Snapshot<state::Canonical> {
    canonical_snapshot(
        snapshot_header(project, "v1.0.0"),
        vec![module_with("demo", &[("greet", 1)])],
    )
}

fn pin(project: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

#[test]
fn for_pin_returns_pinned_state() {
    let _ctx: EvaluationContext<Pinned> = EvaluationContext::for_pin(pin("ra"), snap("ra"));
}

#[test]
fn with_scope_advances_pinned_to_scoped() {
    let snap = snap("ra");
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let ctx: EvaluationContext<Scoped> =
        EvaluationContext::for_pin(pin("ra"), snap).with_scope(scope);
    assert_eq!(ctx.pin().project.as_str(), "ra");
    assert!(ctx.files().is_none());
}

#[test]
fn with_files_advances_scoped_to_sourced() {
    let snap = snap("ra");
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let files =
        EvaluationFiles::new().with(PathBuf::from("src/a.erl"), Some(b"-module(a).".to_vec()));
    let ctx: EvaluationContext<Sourced> =
        EvaluationContext::new(pin("ra"), snap, scope).with_files(files);
    assert!(ctx.files().is_some());
}

#[test]
fn shortcut_new_returns_scoped_state() {
    let snap = snap("ra");
    let scope = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap, []);
    let _ctx: EvaluationContext<Scoped> = EvaluationContext::new(pin("ra"), snap, scope);
}

#[test]
fn evaluation_input_trait_exposes_scope_for_both_states() {
    let snap_a = snap("ra");
    let scope_a = PinScope::from_snapshot(ProjectName::new("ra").unwrap(), &snap_a, []);
    let scoped: EvaluationContext<Scoped> = EvaluationContext::new(pin("ra"), snap_a, scope_a);

    let snap_b = snap("rb");
    let scope_b = PinScope::from_snapshot(ProjectName::new("rb").unwrap(), &snap_b, []);
    let sourced: EvaluationContext<Sourced> =
        EvaluationContext::new(pin("rb"), snap_b, scope_b).with_files(EvaluationFiles::new());

    fn read<C: EvaluationInput>(c: &C) -> &str {
        c.pin().project.as_str()
    }

    assert_eq!(read(&scoped), "ra");
    assert_eq!(read(&sourced), "rb");
}
