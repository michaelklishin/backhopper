// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Whole-program call-graph primitives for `backhopper-xref`.
//!
//! This crate has no Erlang knowledge. It provides:
//!
//! * [`Vertex`] and [`FunctionRef`]: entities the graph tracks
//! * [`VertexSet`] and [`Relation`]: finite sets and binary relations with
//!   the operators a call-graph analysis needs (`union`, `intersection`,
//!   `compose`, `image`, `transitive_closure`, …)
//! * [`CallGraph`]: the aggregate, parameterised on a mode marker
//!   ([`Functions`] vs [`Modules`]) and a phase marker ([`Building`]
//!   vs [`Built`]) so misuses become compile errors
//!
//! See `~/Development/md/backhopper/xref_rs_design.md` for the full design.

pub mod call;
pub mod errors;
pub mod graph;
pub mod loc;
pub mod relation;
pub mod state;
pub mod vertex;

pub use call::{
    CallKind, CallTarget, FunctionRef, FunctionSig, LocalFunctionRef, UnresolvedCallSite,
};
pub use errors::GraphError;
pub use graph::{CallGraph, Deprecation, DeprecationTier, ModuleSummary};
pub use loc::{Loc, PathId, PathInterner, Position};
pub use relation::{Relation, VertexSet};
pub use state::{Building, Built, Functions, Mode, Modules, Phase};
pub use vertex::Vertex;
