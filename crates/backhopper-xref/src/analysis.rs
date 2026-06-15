// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Predefined analyses.
//!
//! Each method on [`Xref`] returns a typed [`result`](crate::result) struct
//! with location-tagged entries.

use std::collections::{BTreeMap, BTreeSet};
use std::iter;

use backhopper_core::{ApplicationName, BehaviourName, Mfa, ModuleName};
use backhopper_xref_graph::{
    CallKind, Deprecation, FunctionRef, FunctionSig, Functions, Loc, Mode, PathId, Position,
    Relation, Vertex, VertexSet,
};

use crate::result::{
    AppDependencies, CalleeEntry, CalleesOf, CallerEntry, CallersOf, DeprecatedCall,
    DeprecatedFunctionCalls, ExportsNotUsed, LocalsNotUsed, ModuleCallers, ModuleDependencies,
    NonConformance, UndefinedFunctionCall, UndefinedFunctionCalls, UnresolvedCalls, UnusedExport,
    UnusedLocal,
};
use crate::xref::Xref;

fn sig_to_mfa(module: &ModuleName, sig: &FunctionSig) -> Mfa {
    Mfa::new(module.clone(), sig.name.clone(), sig.arity)
}

impl Xref<Functions> {
    /// External calls whose target is not defined in any loaded module and
    /// is not a builtin.
    pub fn undefined_function_calls(&self) -> UndefinedFunctionCalls {
        let defined = self.graph().defined_functions();
        let builtins = self.graph().builtins();
        let mut grouped: BTreeMap<(Mfa, Mfa), Vec<Loc>> = BTreeMap::new();
        for (src, tgt) in self.graph().external_calls().iter() {
            let (Vertex::Function(caller), Vertex::Function(callee)) = (src, tgt) else {
                continue;
            };
            if defined.contains(tgt) || builtins.contains(tgt) {
                continue;
            }
            // The callee is undefined, so it has no definition site. Point at
            // the caller's definition instead: that is the function to inspect.
            grouped
                .entry((caller.clone(), callee.clone()))
                .or_default()
                .extend(self.graph().def_at(caller).cloned());
        }
        let entries = grouped
            .into_iter()
            .map(|((caller, callee), locations)| UndefinedFunctionCall {
                caller,
                callee: FunctionRef::Concrete(callee),
                kind: CallKind::Direct,
                locations,
            })
            .collect();
        UndefinedFunctionCalls { entries }
    }

    /// Distinct undefined MFAs (no call-site information).
    pub fn undefined_functions(&self) -> BTreeSet<Mfa> {
        let defined = self.graph().defined_functions();
        let builtins = self.graph().builtins();
        let mut out = BTreeSet::new();
        for (_, tgt) in self.graph().external_calls().iter() {
            if !defined.contains(tgt) && !builtins.contains(tgt) {
                if let Vertex::Function(mfa) = tgt {
                    out.insert(mfa.clone());
                }
            }
        }
        out
    }

    /// Exports that no other module calls.
    pub fn exports_not_used(&self) -> ExportsNotUsed {
        let used_externally = self.graph().external_calls().targets();
        let mut entries = Vec::new();
        for (module, summary) in self.graph().modules() {
            for sig in &summary.exports {
                let mfa = sig_to_mfa(module, sig);
                let v = Vertex::Function(mfa.clone());
                if used_externally.contains(&v) {
                    continue;
                }
                let is_callback = summary.callbacks_required.contains(sig)
                    || summary.callbacks_optional.contains(sig)
                    || self.satisfies_any_callback(module, sig);
                let is_on_load = self.graph().on_load_functions().contains(&mfa);
                let def_loc = self.graph().def_at(&mfa).cloned();
                entries.push(UnusedExport {
                    mfa,
                    def_loc,
                    satisfies_callback: is_callback,
                    is_on_load,
                });
            }
        }
        ExportsNotUsed { entries }
    }

    fn satisfies_any_callback(&self, module: &ModuleName, sig: &FunctionSig) -> bool {
        let Some(summary) = self.graph().module(module) else {
            return false;
        };
        for b in &summary.behaviours {
            let beh_module = ModuleName::new(b.as_str().to_owned()).ok();
            if let Some(bm) = beh_module {
                if let Some(beh_summary) = self.graph().module(&bm) {
                    if beh_summary.callbacks_required.contains(sig)
                        || beh_summary.callbacks_optional.contains(sig)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Locals that are not called from any function in the same module.
    pub fn locals_not_used(&self) -> LocalsNotUsed {
        let used_locally = self.graph().local_calls().targets();
        let mut entries = Vec::new();
        for (module, summary) in self.graph().modules() {
            for sig in &summary.locals {
                let mfa = sig_to_mfa(module, sig);
                let v = Vertex::Function(mfa.clone());
                if used_locally.contains(&v) {
                    continue;
                }
                if self.graph().on_load_functions().contains(&mfa) {
                    continue;
                }
                entries.push(UnusedLocal {
                    mfa: mfa.clone(),
                    def_loc: self.graph().def_at(&mfa).cloned(),
                });
            }
        }
        LocalsNotUsed { entries }
    }

    /// External calls whose target is marked deprecated.
    pub fn deprecated_function_calls(&self) -> DeprecatedFunctionCalls {
        let mut grouped: BTreeMap<(Mfa, Mfa), Vec<Loc>> = BTreeMap::new();
        let deprecated: BTreeMap<Mfa, Deprecation> = self
            .graph()
            .deprecated_mfas()
            .map(|(m, d)| (m.clone(), d.clone()))
            .collect();
        for (src, tgt) in self.graph().external_calls().iter() {
            let (Vertex::Function(caller), Vertex::Function(callee)) = (src, tgt) else {
                continue;
            };
            if !deprecated.contains_key(callee) {
                continue;
            }
            grouped
                .entry((caller.clone(), callee.clone()))
                .or_default()
                .push(
                    self.graph()
                        .def_at(callee)
                        .cloned()
                        .unwrap_or_else(|| Loc::point(PathId::new(0), Position::zero())),
                );
        }
        let entries = grouped
            .into_iter()
            .map(|((caller, callee), locations)| {
                let d = &deprecated[&callee];
                DeprecatedCall {
                    caller,
                    callee,
                    tier: d.tier,
                    message: d.message.clone(),
                    locations,
                }
            })
            .collect();
        DeprecatedFunctionCalls { entries }
    }

    /// Functions that call into `target`. With `transitive`, the preimage
    /// under the transitive closure of the call relation.
    pub fn called_by(&self, target: &Mfa, transitive: bool) -> CallersOf {
        let target_set: VertexSet = iter::once(Vertex::Function(target.clone())).collect();
        let direct;
        let relation: &Relation = if transitive {
            self.all_calls_closure()
        } else {
            direct = self.graph().all_calls();
            &direct
        };
        let sources = relation.preimage(&target_set);
        let entries = sources
            .iter()
            .filter_map(|v| {
                if let Vertex::Function(mfa) = v {
                    Some(CallerEntry {
                        caller: mfa.clone(),
                        kind: CallKind::Direct,
                        locations: vec![],
                    })
                } else {
                    None
                }
            })
            .collect();
        CallersOf {
            target: target.clone(),
            entries,
        }
    }

    /// Functions that `source` calls.
    pub fn calls_from(&self, source: &Mfa, transitive: bool) -> CalleesOf {
        let src_set: VertexSet = iter::once(Vertex::Function(source.clone())).collect();
        let direct;
        let relation: &Relation = if transitive {
            self.all_calls_closure()
        } else {
            direct = self.graph().all_calls();
            &direct
        };
        let targets = relation.image(&src_set);
        let entries = targets
            .iter()
            .filter_map(|v| {
                if let Vertex::Function(mfa) = v {
                    Some(CalleeEntry {
                        callee: FunctionRef::Concrete(mfa.clone()),
                        kind: CallKind::Direct,
                        locations: vec![],
                    })
                } else {
                    None
                }
            })
            .collect();
        CalleesOf {
            source: source.clone(),
            entries,
        }
    }

    /// All recorded unresolved call sites.
    pub fn unresolved_calls(&self) -> UnresolvedCalls {
        UnresolvedCalls {
            entries: self.graph().unresolved_calls().iter().cloned().collect(),
        }
    }

    /// Functions that appear in their own transitive call set.
    pub fn recursive_functions(&self) -> BTreeSet<Mfa> {
        let closure = self.all_calls_closure();
        let mut out = BTreeSet::new();
        for (s, t) in closure.iter() {
            if s == t {
                if let Vertex::Function(mfa) = s {
                    out.insert(mfa.clone());
                }
            }
        }
        out
    }
}

impl<M: Mode> Xref<M> {
    /// Modules that depend on `target` (one-hop, module-level).
    pub fn module_called_by(&self, target: &ModuleName) -> ModuleCallers {
        let target_set: VertexSet = iter::once(Vertex::Module(target.clone())).collect();
        let mods = self.graph().module_edges().preimage(&target_set);
        let entries = mods.iter().filter_map(|v| v.as_module().cloned()).collect();
        ModuleCallers {
            target: target.clone(),
            entries,
        }
    }

    pub fn module_call(&self, source: &ModuleName) -> ModuleDependencies {
        let src_set: VertexSet = iter::once(Vertex::Module(source.clone())).collect();
        let mods = self.graph().module_edges().image(&src_set);
        let entries = mods.iter().filter_map(|v| v.as_module().cloned()).collect();
        ModuleDependencies {
            source: source.clone(),
            entries,
        }
    }

    pub fn implementers_of(&self, behaviour: &BehaviourName) -> BTreeSet<ModuleName> {
        let set: VertexSet = iter::once(Vertex::Behaviour(behaviour.clone())).collect();
        self.graph()
            .behaviour_edges()
            .preimage(&set)
            .iter()
            .filter_map(|v| v.as_module().cloned())
            .collect()
    }

    pub fn nonconformant_implementers(&self) -> Vec<NonConformance> {
        let mut out = Vec::new();
        for (module, summary) in self.graph().modules() {
            for b in &summary.behaviours {
                let beh_module = ModuleName::new(b.as_str().to_owned()).ok();
                let Some(beh) = beh_module.and_then(|m| self.graph().module(&m)) else {
                    continue;
                };
                let mut missing = BTreeSet::new();
                for required in &beh.callbacks_required {
                    if !summary.exports.contains(required) {
                        missing.insert(required.clone());
                    }
                }
                if !missing.is_empty() {
                    out.push(NonConformance {
                        implementer: module.clone(),
                        behaviour: b.clone(),
                        missing,
                    });
                }
            }
        }
        out
    }

    /// Strongly connected components of the module call graph that contain
    /// more than one module.
    pub fn module_cycles(&self) -> Vec<Vec<ModuleName>> {
        let closure = self.graph().module_edges().reflexive_transitive_closure();
        let mut groups: BTreeMap<ModuleName, BTreeSet<ModuleName>> = BTreeMap::new();
        for (s, t) in closure.iter() {
            if s == t {
                continue;
            }
            if closure.contains(t, s) {
                if let (Some(sm), Some(tm)) = (s.as_module(), t.as_module()) {
                    groups.entry(sm.clone()).or_default().insert(tm.clone());
                }
            }
        }
        let mut seen: BTreeSet<ModuleName> = BTreeSet::new();
        let mut out = Vec::new();
        for (m, members) in groups {
            if seen.contains(&m) {
                continue;
            }
            let mut full = members;
            full.insert(m);
            for x in &full {
                seen.insert(x.clone());
            }
            let mut cycle: Vec<ModuleName> = full.into_iter().collect();
            cycle.sort();
            out.push(cycle);
        }
        out
    }

    /// Applications a module's edges reach.
    pub fn application_call(&self, source: &ApplicationName) -> AppDependencies {
        let set: VertexSet = iter::once(Vertex::Application(source.clone())).collect();
        let entries: BTreeSet<ApplicationName> = self
            .graph()
            .application_edges()
            .image(&set)
            .iter()
            .filter_map(|v| v.as_application().cloned())
            .collect();
        AppDependencies {
            source: source.clone(),
            entries,
        }
    }
}
