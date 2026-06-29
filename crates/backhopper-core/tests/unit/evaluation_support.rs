// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Helpers shared by the `evaluation_*` unit test files: a canonical
//! snapshot for a project and a ready-to-evaluate context.

use backhopper_core::compat::patch::EvaluationContext;
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::ProjectName;
use backhopper_core::model::snapshot::{Module, Snapshot, state};
use backhopper_test_support::{canonical_snapshot, pin, snapshot_header};

pub(crate) fn snapshot(project: &str, modules: Vec<Module>) -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header(project, "v1.0.0"), modules)
}

pub(crate) fn make_context(project: &str, modules_in_snapshot: Vec<Module>) -> EvaluationContext {
    let snap = snapshot(project, modules_in_snapshot);
    let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
    EvaluationContext::new(pin(project, "v1.0.0"), snap, scope)
}
