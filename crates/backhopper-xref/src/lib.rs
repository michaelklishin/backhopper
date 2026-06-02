// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Whole-program cross-reference analysis for Erlang projects.
//!
//! This is the consumer-facing façade. Build an [`Xref`] with
//! [`XrefBuilder`], then run [predefined analyses](analysis) (undefined-call
//! detection, dead-export detection, behaviour conformance, reverse
//! traversal, cycle detection, and [`XrefDiff`] across
//! two tags).
//!
//! The test-suite-selection adapter [`suites`] consumes the graph to
//! answer "which `_SUITE` modules should I run for this diff?".

pub mod analysis;
pub mod builder;
pub mod diff;
pub mod display;
pub mod errors;
pub mod result;
pub mod suites;
pub mod xref;

pub use builder::XrefBuilder;
pub use diff::{XrefDiff, diff_xrefs};
pub use errors::XrefError;
pub use result::{
    AnalysisResult, AppDependencies, CalleeEntry, CalleesOf, CallerEntry, CallersOf,
    DeprecatedCall, DeprecatedFunctionCalls, ExportsNotUsed, LocalsNotUsed, ModuleCallers,
    ModuleDependencies, NonConformance, UndefinedFunctionCall, UndefinedFunctionCalls,
    UnresolvedCalls, UnusedExport, UnusedLocal,
};
pub use suites::{SuiteRef, is_suite_module, suites_referencing, suites_referencing_mfas};
pub use xref::Xref;

pub use backhopper_xref_graph::{
    CallGraph, CallKind, CallTarget, Deprecation, DeprecationTier, FunctionRef, FunctionSig,
    Functions, Loc, Modules, Position, Relation, Vertex, VertexSet,
};
pub use backhopper_xref_reader::{
    ApplicationAssignment, ModuleData, ProjectLayout, ReadError, ReadOutput, ReadWarning,
    SourceReader,
};
