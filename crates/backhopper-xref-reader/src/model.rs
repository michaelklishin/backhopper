// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `ModuleData`, `CallSite`, and `ReadOutput` aggregate types.

use std::collections::{BTreeMap, BTreeSet};

use backhopper_core::{BehaviourName, Mfa, ModuleName};
use backhopper_xref_graph::{
    CallKind, CallTarget, Deprecation, FunctionSig, Loc, PathInterner, UnresolvedCallSite,
};

use crate::application::ApplicationAssignment;
use crate::errors::ReadWarning;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub caller: FunctionSig,
    pub callee: CallTarget,
    pub kind: CallKind,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleData {
    pub module: ModuleName,
    pub application: ApplicationAssignment,

    pub exports: BTreeSet<FunctionSig>,
    pub locals: BTreeSet<FunctionSig>,
    pub external_calls: Vec<CallSite>,
    pub local_calls: Vec<CallSite>,
    pub unresolved: BTreeSet<UnresolvedCallSite>,

    pub on_load: Option<FunctionSig>,
    pub deprecated: BTreeMap<FunctionSig, Deprecation>,

    pub behaviours: BTreeSet<BehaviourName>,
    pub callbacks: BTreeSet<FunctionSig>,
    pub optional_callbacks: BTreeSet<FunctionSig>,

    pub imports: BTreeMap<ModuleName, BTreeSet<FunctionSig>>,
    pub definitions: BTreeMap<FunctionSig, Loc>,
}

impl ModuleData {
    pub fn function_mfa(&self, sig: &FunctionSig) -> Mfa {
        Mfa {
            module: self.module.clone(),
            function: sig.name.clone(),
            arity: sig.arity,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReadOutput {
    pub modules: Vec<ModuleData>,
    pub warnings: Vec<ReadWarning>,
    pub paths: PathInterner,
}
