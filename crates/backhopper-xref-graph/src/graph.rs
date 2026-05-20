// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The aggregate `CallGraph` type.
//!
//! Mode (`Functions` vs `Modules`) and phase (`Building` vs `Built`) are
//! lifted to the type system: `exports_not_used` is a method on
//! `CallGraph<Functions, Built>` only, so a misuse against a modules-mode
//! or under-construction graph fails at compile time.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use backhopper_core::{ApplicationName, BehaviourName, Mfa, ModuleName};
use serde::{Deserialize, Serialize};

use crate::call::{FunctionSig, UnresolvedCallSite};
use crate::loc::Loc;
use crate::relation::{Relation, VertexSet};
use crate::state::{Building, Built, Functions, Mode, Phase};
use crate::vertex::Vertex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeprecationTier {
    Now,
    Next,
    Eventual,
}

impl fmt::Display for DeprecationTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Now => "now",
            Self::Next => "next_release",
            Self::Eventual => "eventual",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deprecation {
    pub tier: DeprecationTier,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleSummary {
    pub application: Option<ApplicationName>,
    pub exports: BTreeSet<FunctionSig>,
    pub locals: BTreeSet<FunctionSig>,
    pub on_load: Option<FunctionSig>,
    pub behaviours: BTreeSet<BehaviourName>,
    pub callbacks_required: BTreeSet<FunctionSig>,
    pub callbacks_optional: BTreeSet<FunctionSig>,
}

#[derive(Debug, Clone)]
pub struct CallGraph<M: Mode, P: Phase = Built> {
    mode: PhantomData<M>,
    phase: PhantomData<P>,

    modules: BTreeMap<ModuleName, ModuleSummary>,
    def_at: BTreeMap<Mfa, Loc>,
    deprecated: BTreeMap<Mfa, Deprecation>,
    on_load_functions: BTreeSet<Mfa>,

    local_calls: Relation,
    external_calls: Relation,
    module_edges: Relation,
    application_edges: Relation,
    behaviour_edges: Relation,

    unresolved: BTreeSet<UnresolvedCallSite>,
    builtins: VertexSet,
}

impl<M: Mode, P: Phase> CallGraph<M, P> {
    fn empty() -> Self {
        Self {
            mode: PhantomData,
            phase: PhantomData,
            modules: BTreeMap::new(),
            def_at: BTreeMap::new(),
            deprecated: BTreeMap::new(),
            on_load_functions: BTreeSet::new(),
            local_calls: Relation::new(),
            external_calls: Relation::new(),
            module_edges: Relation::new(),
            application_edges: Relation::new(),
            behaviour_edges: Relation::new(),
            unresolved: BTreeSet::new(),
            builtins: VertexSet::new(),
        }
    }
}

impl<M: Mode> CallGraph<M, Building> {
    pub fn new() -> Self {
        Self::empty()
    }

    pub fn set_builtins(&mut self, b: VertexSet) -> &mut Self {
        self.builtins = b;
        self
    }

    pub fn insert_module(&mut self, name: ModuleName, summary: ModuleSummary) {
        let app_vertex = summary.application.clone().map(Vertex::Application);
        for b in &summary.behaviours {
            self.behaviour_edges
                .insert(Vertex::Module(name.clone()), Vertex::Behaviour(b.clone()));
            if let Some(a) = &app_vertex {
                self.application_edges
                    .insert(a.clone(), Vertex::Behaviour(b.clone()));
            }
        }
        self.modules.insert(name, summary);
    }

    pub fn record_definition(&mut self, mfa: Mfa, loc: Loc) {
        self.def_at.insert(mfa, loc);
    }

    pub fn record_on_load(&mut self, mfa: Mfa) {
        self.on_load_functions.insert(mfa);
    }

    pub fn record_deprecation(&mut self, mfa: Mfa, d: Deprecation) {
        self.deprecated.insert(mfa, d);
    }

    pub fn insert_local_call(&mut self, caller: Mfa, callee: Mfa) {
        self.local_calls
            .insert(Vertex::Function(caller), Vertex::Function(callee));
    }

    pub fn insert_external_call(&mut self, caller: Mfa, callee: Mfa) {
        let caller_mod = caller.module.clone();
        let callee_mod = callee.module.clone();
        self.external_calls
            .insert(Vertex::Function(caller), Vertex::Function(callee));
        if caller_mod != callee_mod {
            let caller_app = self
                .modules
                .get(&caller_mod)
                .and_then(|m| m.application.clone());
            let callee_app = self
                .modules
                .get(&callee_mod)
                .and_then(|m| m.application.clone());
            self.module_edges
                .insert(Vertex::Module(caller_mod), Vertex::Module(callee_mod));
            if let (Some(a), Some(b)) = (caller_app, callee_app)
                && a != b
            {
                self.application_edges
                    .insert(Vertex::Application(a), Vertex::Application(b));
            }
        }
    }

    pub fn insert_unresolved(&mut self, site: UnresolvedCallSite) {
        self.unresolved.insert(site);
    }

    pub fn finish(self) -> CallGraph<M, Built> {
        CallGraph {
            mode: PhantomData,
            phase: PhantomData,
            modules: self.modules,
            def_at: self.def_at,
            deprecated: self.deprecated,
            on_load_functions: self.on_load_functions,
            local_calls: self.local_calls,
            external_calls: self.external_calls,
            module_edges: self.module_edges,
            application_edges: self.application_edges,
            behaviour_edges: self.behaviour_edges,
            unresolved: self.unresolved,
            builtins: self.builtins,
        }
    }
}

impl<M: Mode> Default for CallGraph<M, Building> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Mode> CallGraph<M, Built> {
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleName, &ModuleSummary)> {
        self.modules.iter()
    }

    pub fn module(&self, name: &ModuleName) -> Option<&ModuleSummary> {
        self.modules.get(name)
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    pub fn def_at(&self, mfa: &Mfa) -> Option<&Loc> {
        self.def_at.get(mfa)
    }

    pub fn deprecation(&self, mfa: &Mfa) -> Option<&Deprecation> {
        self.deprecated.get(mfa)
    }

    pub fn deprecated_mfas(&self) -> impl Iterator<Item = (&Mfa, &Deprecation)> {
        self.deprecated.iter()
    }

    pub fn on_load_functions(&self) -> &BTreeSet<Mfa> {
        &self.on_load_functions
    }

    pub fn builtins(&self) -> &VertexSet {
        &self.builtins
    }

    pub fn module_edges(&self) -> &Relation {
        &self.module_edges
    }

    pub fn application_edges(&self) -> &Relation {
        &self.application_edges
    }

    pub fn behaviour_edges(&self) -> &Relation {
        &self.behaviour_edges
    }

    pub fn unresolved_calls(&self) -> &BTreeSet<UnresolvedCallSite> {
        &self.unresolved
    }

    pub fn exports_as_vertices(&self) -> VertexSet {
        self.modules
            .iter()
            .flat_map(|(m, s)| s.exports.iter().map(move |f| function_vertex(m, f)))
            .collect()
    }

    pub fn defined_functions(&self) -> VertexSet {
        self.modules
            .iter()
            .flat_map(|(m, s)| {
                s.exports
                    .iter()
                    .chain(s.locals.iter())
                    .map(move |f| function_vertex(m, f))
            })
            .collect()
    }
}

fn function_vertex(module: &ModuleName, sig: &FunctionSig) -> Vertex {
    Vertex::Function(Mfa::new(module.clone(), sig.name.clone(), sig.arity))
}

impl CallGraph<Functions, Built> {
    pub fn local_calls(&self) -> &Relation {
        &self.local_calls
    }

    pub fn external_calls(&self) -> &Relation {
        &self.external_calls
    }

    pub fn all_calls(&self) -> Relation {
        self.local_calls.union(&self.external_calls)
    }

    pub fn locals_as_vertices(&self) -> VertexSet {
        self.modules
            .iter()
            .flat_map(|(m, s)| s.locals.iter().map(move |f| function_vertex(m, f)))
            .collect()
    }
}
