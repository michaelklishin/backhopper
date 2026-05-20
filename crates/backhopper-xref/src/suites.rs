// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Test-suite-selection adapter.
//!
//! The R3, R4, and R5 rules in
//! `~/Development/md/backports/test_suite_selection.md` all reduce to:
//! given a set of modified modules `S`, find suites that reference them.
//! With the xref module call graph this is one reverse-traversal query.

use std::collections::BTreeSet;

use backhopper_core::{ApplicationName, Mfa, ModuleName};
use backhopper_xref_graph::{Functions, Vertex, VertexSet};
use serde::{Deserialize, Serialize};

use crate::xref::Xref;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SuiteRef {
    pub application: Option<ApplicationName>,
    pub module: ModuleName,
}

/// Modules that call into any of `modules` and match `suite_predicate`.
///
/// Useful for R3, R4, R5 from the test-suite-selection rules: pass the
/// modified-modules set and a predicate that recognises `_SUITE` modules.
pub fn suites_referencing(
    xref: &Xref<Functions>,
    modules: &BTreeSet<ModuleName>,
    suite_predicate: impl Fn(&ModuleName) -> bool,
) -> BTreeSet<SuiteRef> {
    let targets: VertexSet = modules.iter().map(|m| Vertex::Module(m.clone())).collect();
    let callers = xref.graph().module_edges().preimage(&targets);
    callers
        .iter()
        .filter_map(|v| v.as_module().cloned())
        .filter(|m| suite_predicate(m))
        .map(|module| {
            let application = xref
                .graph()
                .module(&module)
                .and_then(|s| s.application.clone());
            SuiteRef {
                application,
                module,
            }
        })
        .collect()
}

/// Suites that call any of the listed MFAs.
pub fn suites_referencing_mfas(
    xref: &Xref<Functions>,
    targets: &BTreeSet<Mfa>,
    suite_predicate: impl Fn(&ModuleName) -> bool,
) -> BTreeSet<SuiteRef> {
    let target_vs: VertexSet = targets
        .iter()
        .map(|m| Vertex::Function(m.clone()))
        .collect();
    let callers = xref.graph().external_calls().preimage(&target_vs);
    let mut suite_modules: BTreeSet<ModuleName> = BTreeSet::new();
    for v in callers.iter() {
        if let Vertex::Function(mfa) = v {
            if suite_predicate(&mfa.module) {
                suite_modules.insert(mfa.module.clone());
            }
        }
    }
    suite_modules
        .into_iter()
        .map(|module| {
            let application = xref
                .graph()
                .module(&module)
                .and_then(|s| s.application.clone());
            SuiteRef {
                application,
                module,
            }
        })
        .collect()
}

/// Default suite predicate: module name ends with `_SUITE`.
pub fn is_suite_module(name: &ModuleName) -> bool {
    name.as_str().ends_with("_SUITE")
}