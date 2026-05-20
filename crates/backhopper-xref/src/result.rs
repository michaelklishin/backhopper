// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Typed result data for each predefined analysis.
//!
//! Every result type carries the original call-site locations so consumers
//! can format human-readable output without re-walking source. The
//! `Display` impls live in `display.rs`.

use std::collections::BTreeSet;

use backhopper_core::{ApplicationName, BehaviourName, Mfa, ModuleName};
use backhopper_xref_graph::{
    CallKind, DeprecationTier, FunctionRef, FunctionSig, Loc, UnresolvedCallSite,
};
use serde::{Deserialize, Serialize};

pub trait AnalysisResult: serde::Serialize {
    fn is_clean(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndefinedFunctionCall {
    pub caller: Mfa,
    pub callee: FunctionRef,
    pub kind: CallKind,
    pub locations: Vec<Loc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndefinedFunctionCalls {
    pub entries: Vec<UndefinedFunctionCall>,
}

impl AnalysisResult for UndefinedFunctionCalls {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnusedLocal {
    pub mfa: Mfa,
    pub def_loc: Option<Loc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalsNotUsed {
    pub entries: Vec<UnusedLocal>,
}

impl AnalysisResult for LocalsNotUsed {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnusedExport {
    pub mfa: Mfa,
    pub def_loc: Option<Loc>,
    pub satisfies_callback: bool,
    pub is_on_load: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportsNotUsed {
    pub entries: Vec<UnusedExport>,
}

impl AnalysisResult for ExportsNotUsed {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeprecatedCall {
    pub caller: Mfa,
    pub callee: Mfa,
    pub tier: DeprecationTier,
    pub message: Option<String>,
    pub locations: Vec<Loc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeprecatedFunctionCalls {
    pub entries: Vec<DeprecatedCall>,
}

impl AnalysisResult for DeprecatedFunctionCalls {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerEntry {
    pub caller: Mfa,
    pub kind: CallKind,
    pub locations: Vec<Loc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallersOf {
    pub target: Mfa,
    pub entries: Vec<CallerEntry>,
}

impl AnalysisResult for CallersOf {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalleeEntry {
    pub callee: FunctionRef,
    pub kind: CallKind,
    pub locations: Vec<Loc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalleesOf {
    pub source: Mfa,
    pub entries: Vec<CalleeEntry>,
}

impl AnalysisResult for CalleesOf {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleCallers {
    pub target: ModuleName,
    pub entries: BTreeSet<ModuleName>,
}

impl AnalysisResult for ModuleCallers {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDependencies {
    pub source: ModuleName,
    pub entries: BTreeSet<ModuleName>,
}

impl AnalysisResult for ModuleDependencies {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppDependencies {
    pub source: ApplicationName,
    pub entries: BTreeSet<ApplicationName>,
}

impl AnalysisResult for AppDependencies {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NonConformance {
    pub implementer: ModuleName,
    pub behaviour: BehaviourName,
    pub missing: BTreeSet<FunctionSig>,
}

impl AnalysisResult for NonConformance {
    fn is_clean(&self) -> bool {
        self.missing.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedCalls {
    pub entries: Vec<UnresolvedCallSite>,
}

impl AnalysisResult for UnresolvedCalls {
    fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}