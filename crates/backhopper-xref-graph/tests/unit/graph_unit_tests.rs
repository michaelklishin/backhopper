// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;

use backhopper_core::{ApplicationName, Arity, BehaviourName, FunctionName, Mfa, ModuleName};
use backhopper_xref_graph::{
    CallGraph, Deprecation, DeprecationTier, FunctionSig, Functions, Loc, ModuleSummary, PathId,
    Position, Vertex,
};

fn fname(s: &str) -> FunctionName {
    FunctionName::new(s.to_owned()).unwrap()
}

fn mname(s: &str) -> ModuleName {
    ModuleName::new(s.to_owned()).unwrap()
}

fn mfa(m: &str, f: &str, a: u8) -> Mfa {
    Mfa::new(mname(m), fname(f), Arity::new(a))
}

fn sig(f: &str, a: u8) -> FunctionSig {
    FunctionSig::new(fname(f), Arity::new(a))
}

fn point() -> Loc {
    Loc::point(PathId::new(0), Position::zero())
}

#[test]
fn empty_built_graph_has_no_modules() {
    let g: CallGraph<Functions, _> = CallGraph::new().finish();
    assert_eq!(g.module_count(), 0);
    assert!(g.all_calls().is_empty());
}

#[test]
fn module_summary_round_trips_through_builder() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let mut summary = ModuleSummary::default();
    summary.exports.insert(sig("append", 1));
    summary.locals.insert(sig("fold", 0));
    g.insert_module(mname("ra_log"), summary);
    let g = g.finish();
    let s = g.module(&mname("ra_log")).unwrap();
    assert_eq!(s.exports.len(), 1);
    assert_eq!(s.locals.len(), 1);
}

#[test]
fn external_call_creates_module_edge() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_server"), ModuleSummary::default());
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_external_call(mfa("ra_server", "process", 0), mfa("ra_log", "append", 1));
    let g = g.finish();
    let me = g.module_edges();
    assert!(me.contains(
        &Vertex::Module(mname("ra_server")),
        &Vertex::Module(mname("ra_log"))
    ));
}

#[test]
fn external_call_within_same_module_does_not_create_module_edge() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_external_call(mfa("ra_log", "append", 0), mfa("ra_log", "fold", 0));
    let g = g.finish();
    assert!(g.module_edges().is_empty());
}

#[test]
fn external_call_across_applications_creates_app_edge() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let ra = ApplicationName::new("ra".to_owned()).unwrap();
    let khepri = ApplicationName::new("khepri".to_owned()).unwrap();
    let s1 = ModuleSummary {
        application: Some(ra.clone()),
        ..ModuleSummary::default()
    };
    let s2 = ModuleSummary {
        application: Some(khepri.clone()),
        ..ModuleSummary::default()
    };
    g.insert_module(mname("ra_server"), s1);
    g.insert_module(mname("khepri_machine"), s2);
    g.insert_external_call(
        mfa("ra_server", "handle", 0),
        mfa("khepri_machine", "get", 0),
    );
    let g = g.finish();
    assert!(
        g.application_edges()
            .contains(&Vertex::Application(ra), &Vertex::Application(khepri))
    );
}

#[test]
fn behaviour_declaration_creates_module_to_behaviour_edge() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let mut summary = ModuleSummary::default();
    let b = BehaviourName::new("gen_server".to_owned()).unwrap();
    summary.behaviours.insert(b.clone());
    g.insert_module(mname("ra_server_proc"), summary);
    let g = g.finish();
    assert!(g.behaviour_edges().contains(
        &Vertex::Module(mname("ra_server_proc")),
        &Vertex::Behaviour(b)
    ));
}

#[test]
fn deprecation_round_trips() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let f = mfa("ra_lib", "dump", 0);
    g.record_deprecation(
        f.clone(),
        Deprecation {
            tier: DeprecationTier::Next,
            message: Some("use ra_lib:id/1".to_owned()),
        },
    );
    let g = g.finish();
    let d = g.deprecation(&f).unwrap();
    assert_eq!(d.tier, DeprecationTier::Next);
    assert_eq!(d.message.as_deref(), Some("use ra_lib:id/1"));
    assert_eq!(g.deprecated_mfas().count(), 1);
}

#[test]
fn on_load_round_trips() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let f = mfa("ra_counters", "init", 0);
    g.record_on_load(f.clone());
    let g = g.finish();
    assert!(g.on_load_functions().contains(&f));
}

#[test]
fn definitions_carry_location() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let f = mfa("ra_log", "append", 0);
    let loc = point();
    g.record_definition(f.clone(), loc.clone());
    let g = g.finish();
    assert_eq!(g.def_at(&f), Some(&loc));
}

#[test]
fn defined_functions_combines_exports_and_locals() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let mut summary = ModuleSummary::default();
    summary.exports.insert(sig("append", 0));
    summary.locals.insert(sig("fold", 1));
    g.insert_module(mname("ra_log"), summary);
    let g = g.finish();
    let d = g.defined_functions();
    assert_eq!(d.len(), 2);
}

#[test]
fn local_calls_kept_separate_from_external() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_local_call(mfa("ra_log", "append", 0), mfa("ra_log", "fold", 0));
    let g = g.finish();
    assert_eq!(g.local_calls().len(), 1);
    assert!(g.external_calls().is_empty());
}

#[test]
fn all_calls_unions_local_and_external() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_module(mname("ra_machine"), ModuleSummary::default());
    g.insert_local_call(mfa("ra_log", "append", 0), mfa("ra_log", "fold", 0));
    g.insert_external_call(mfa("ra_log", "append", 0), mfa("ra_machine", "apply", 0));
    let g = g.finish();
    assert_eq!(g.all_calls().len(), 2);
}

#[test]
fn module_summary_iteration_is_sorted() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_server"), ModuleSummary::default());
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_module(mname("ra_machine"), ModuleSummary::default());
    let g = g.finish();
    let names: Vec<&str> = g.modules().map(|(m, _)| m.as_str()).collect();
    assert_eq!(names, vec!["ra_log", "ra_machine", "ra_server"]);
}

#[test]
fn deprecated_set_iteration_is_sorted() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    for n in ["id", "ceiling", "dump"] {
        g.record_deprecation(
            mfa("ra_lib", n, 0),
            Deprecation {
                tier: DeprecationTier::Eventual,
                message: None,
            },
        );
    }
    let g = g.finish();
    let names: Vec<&str> = g
        .deprecated_mfas()
        .map(|(m, _)| m.function.as_str())
        .collect();
    assert_eq!(names, vec!["ceiling", "dump", "id"]);
}

#[test]
fn definition_set_handles_empty_module_summary() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    let g = g.finish();
    assert!(g.defined_functions().is_empty());
}

#[test]
fn behaviour_with_application_creates_app_to_behaviour_edge() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let app = ApplicationName::new("rabbit".to_owned()).unwrap();
    let beh = BehaviourName::new("gen_server".to_owned()).unwrap();
    let mut s = ModuleSummary {
        application: Some(app.clone()),
        ..ModuleSummary::default()
    };
    s.behaviours.insert(beh.clone());
    g.insert_module(mname("rabbit_channel"), s);
    let g = g.finish();
    assert!(
        g.application_edges()
            .contains(&Vertex::Application(app), &Vertex::Behaviour(beh))
    );
}

#[test]
fn unresolved_set_starts_empty() {
    let g: CallGraph<Functions, _> = CallGraph::new().finish();
    assert!(g.unresolved_calls().is_empty());
}

#[test]
fn modules_iteration_via_module_accessor() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    g.insert_module(mname("ra_log"), ModuleSummary::default());
    g.insert_module(mname("ra_machine"), ModuleSummary::default());
    let g = g.finish();
    assert!(g.module(&mname("ra_log")).is_some());
    assert!(g.module(&mname("ra_server")).is_none());
}

#[test]
fn building_then_finishing_yields_built_phase() {
    let g: CallGraph<Functions, _> = CallGraph::new();
    let _: CallGraph<Functions, backhopper_xref_graph::Built> = g.finish();
}

#[test]
fn modules_count_matches_insert_count() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    for name in &["ra", "ra_log", "ra_machine", "ra_server", "ra_directory"] {
        g.insert_module(mname(name), ModuleSummary::default());
    }
    let g = g.finish();
    assert_eq!(g.module_count(), 5);
}

#[test]
fn callbacks_required_round_trips() {
    let mut g: CallGraph<Functions, _> = CallGraph::new();
    let mut s = ModuleSummary::default();
    let mut required = BTreeSet::new();
    required.insert(sig("handle_call", 3));
    s.callbacks_required = required;
    g.insert_module(mname("ra_server_proc"), s);
    let g = g.finish();
    assert_eq!(
        g.module(&mname("ra_server_proc"))
            .unwrap()
            .callbacks_required
            .len(),
        1
    );
}
