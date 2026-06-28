// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Xref<M>`: the queryable façade over a built `CallGraph`.
//!
//! Wraps a `CallGraph<M, Built>` plus a few memoised derived views.

use std::sync::{Arc, OnceLock};

use backhopper_xref_graph::{Built, CallGraph, Functions, Mode, Relation};
use backhopper_xref_reader::ReadWarning;

#[derive(Debug, Clone)]
pub struct Xref<M: Mode> {
    pub(crate) graph: Arc<CallGraph<M, Built>>,
    pub(crate) warnings: Arc<Vec<ReadWarning>>,
    pub(crate) closure_memo: OnceLock<Arc<Relation>>,
}

impl<M: Mode> Xref<M> {
    pub(crate) fn from_graph(graph: CallGraph<M, Built>, warnings: Vec<ReadWarning>) -> Self {
        Self {
            graph: Arc::new(graph),
            warnings: Arc::new(warnings),
            closure_memo: OnceLock::new(),
        }
    }

    pub fn graph(&self) -> &CallGraph<M, Built> {
        &self.graph
    }

    pub fn warnings(&self) -> &[ReadWarning] {
        &self.warnings
    }
}

impl Xref<Functions> {
    pub fn reverse_calls(&self) -> Relation {
        self.graph.all_calls().reversed()
    }

    /// Transitive closure of every function call, computed once and
    /// reused: the closure is the dominant cost of reachability queries
    /// and the graph is immutable
    pub(crate) fn all_calls_closure(&self) -> &Relation {
        self.closure_memo
            .get_or_init(|| Arc::new(self.graph.all_calls().transitive_closure()))
    }
}
