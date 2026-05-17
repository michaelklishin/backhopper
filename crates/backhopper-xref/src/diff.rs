//! `XrefDiff`: cross-tag comparison of two built xref graphs.

use std::collections::{BTreeMap, BTreeSet};

use backhopper_core::{BehaviourName, Mfa, ModuleName};
use backhopper_xref_graph::{Deprecation, FunctionRef, Functions, Vertex};
use serde::{Deserialize, Serialize};

use crate::xref::Xref;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct XrefDiff {
    pub added_modules: BTreeSet<ModuleName>,
    pub removed_modules: BTreeSet<ModuleName>,

    pub added_exports: BTreeSet<Mfa>,
    pub removed_exports: BTreeSet<Mfa>,

    pub added_calls: BTreeSet<(Mfa, FunctionRef)>,
    pub removed_calls: BTreeSet<(Mfa, FunctionRef)>,

    pub newly_deprecated: BTreeMap<Mfa, Deprecation>,
    pub un_deprecated: BTreeSet<Mfa>,

    pub added_behaviour_impls: BTreeSet<(ModuleName, BehaviourName)>,
    pub removed_behaviour_impls: BTreeSet<(ModuleName, BehaviourName)>,
}

pub fn diff_xrefs(from: &Xref<Functions>, to: &Xref<Functions>) -> XrefDiff {
    let mut diff = XrefDiff::default();

    let from_modules: BTreeSet<ModuleName> =
        from.graph().modules().map(|(m, _)| m.clone()).collect();
    let to_modules: BTreeSet<ModuleName> = to.graph().modules().map(|(m, _)| m.clone()).collect();
    diff.added_modules = to_modules.difference(&from_modules).cloned().collect();
    diff.removed_modules = from_modules.difference(&to_modules).cloned().collect();

    let from_exports = collect_exports(from);
    let to_exports = collect_exports(to);
    diff.added_exports = to_exports.difference(&from_exports).cloned().collect();
    diff.removed_exports = from_exports.difference(&to_exports).cloned().collect();

    let from_calls = collect_external_calls(from);
    let to_calls = collect_external_calls(to);
    diff.added_calls = to_calls.difference(&from_calls).cloned().collect();
    diff.removed_calls = from_calls.difference(&to_calls).cloned().collect();

    let from_dep: BTreeMap<Mfa, Deprecation> = from
        .graph()
        .deprecated_mfas()
        .map(|(m, d)| (m.clone(), d.clone()))
        .collect();
    let to_dep: BTreeMap<Mfa, Deprecation> = to
        .graph()
        .deprecated_mfas()
        .map(|(m, d)| (m.clone(), d.clone()))
        .collect();
    for (mfa, d) in &to_dep {
        if !from_dep.contains_key(mfa) {
            diff.newly_deprecated.insert(mfa.clone(), d.clone());
        }
    }
    for mfa in from_dep.keys() {
        if !to_dep.contains_key(mfa) {
            diff.un_deprecated.insert(mfa.clone());
        }
    }

    let from_impls = collect_behaviour_impls(from);
    let to_impls = collect_behaviour_impls(to);
    diff.added_behaviour_impls = to_impls.difference(&from_impls).cloned().collect();
    diff.removed_behaviour_impls = from_impls.difference(&to_impls).cloned().collect();

    diff
}

fn collect_exports(x: &Xref<Functions>) -> BTreeSet<Mfa> {
    let mut out = BTreeSet::new();
    for (m, summary) in x.graph().modules() {
        for sig in &summary.exports {
            out.insert(Mfa::new(m.clone(), sig.name.clone(), sig.arity));
        }
    }
    out
}

fn collect_external_calls(x: &Xref<Functions>) -> BTreeSet<(Mfa, FunctionRef)> {
    let mut out = BTreeSet::new();
    for (src, tgt) in x.graph().external_calls().iter() {
        let (Vertex::Function(caller), Vertex::Function(callee)) = (src, tgt) else {
            continue;
        };
        out.insert((caller.clone(), FunctionRef::Concrete(callee.clone())));
    }
    out
}

fn collect_behaviour_impls(x: &Xref<Functions>) -> BTreeSet<(ModuleName, BehaviourName)> {
    let mut out = BTreeSet::new();
    for (s, t) in x.graph().behaviour_edges().iter() {
        if let (Some(module), Some(beh)) = (s.as_module(), t.as_behaviour()) {
            out.insert((module.clone(), beh.clone()));
        }
    }
    out
}
