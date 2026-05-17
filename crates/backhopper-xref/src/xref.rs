//! `Xref<M>`: the queryable façade over a built `CallGraph`.
//!
//! Wraps a `CallGraph<M, Built>` plus a few memoised derived views.

use std::sync::Arc;

use backhopper_xref_graph::{Built, CallGraph, Functions, Mode, Relation};
use backhopper_xref_reader::ReadWarning;

#[derive(Debug, Clone)]
pub struct Xref<M: Mode> {
    pub(crate) graph: Arc<CallGraph<M, Built>>,
    pub(crate) reverse_module_calls: Arc<Relation>,
    pub(crate) warnings: Arc<Vec<ReadWarning>>,
}

impl<M: Mode> Xref<M> {
    pub(crate) fn from_graph(graph: CallGraph<M, Built>, warnings: Vec<ReadWarning>) -> Self {
        let reverse_module_calls = graph.module_edges().reversed();
        Self {
            graph: Arc::new(graph),
            reverse_module_calls: Arc::new(reverse_module_calls),
            warnings: Arc::new(warnings),
        }
    }

    pub fn graph(&self) -> &CallGraph<M, Built> {
        &self.graph
    }

    pub fn reverse_module_calls(&self) -> &Relation {
        &self.reverse_module_calls
    }

    pub fn warnings(&self) -> &[ReadWarning] {
        &self.warnings
    }
}

impl Xref<Functions> {
    pub fn reverse_calls(&self) -> Relation {
        self.graph.all_calls().reversed()
    }
}
