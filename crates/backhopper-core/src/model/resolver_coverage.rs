// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Which symbol classes the analyzer resolves, and the classifier from a
//! symbol to its class.
//!
//! A symbol-presence taxonomy across mechanisms: macro, record, local
//! call, include, and behaviour resolve on the target tree; qualified
//! call and type through the snapshot. The fold routes a break by class
//! against the producing row's coverage: a checked class is a resolver
//! bug, an unchecked one a coverage gap.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::model::symbol::SymbolKind;

/// A class of symbol whose presence a backport can dangle on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResolverClass {
    Macro,
    Include,
    Behaviour,
    QualifiedCall,
    Record,
    LocalCall,
    Type,
    IndirectCall,
}

impl ResolverClass {
    /// Stable label matching the serde spelling, for text roll-ups.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ResolverClass::Macro => "macro",
            ResolverClass::Include => "include",
            ResolverClass::Behaviour => "behaviour",
            ResolverClass::QualifiedCall => "qualified_call",
            ResolverClass::Record => "record",
            ResolverClass::LocalCall => "local_call",
            ResolverClass::Type => "type",
            ResolverClass::IndirectCall => "indirect_call",
        }
    }

    pub const ALL: [ResolverClass; 8] = [
        ResolverClass::Macro,
        ResolverClass::Include,
        ResolverClass::Behaviour,
        ResolverClass::QualifiedCall,
        ResolverClass::Record,
        ResolverClass::LocalCall,
        ResolverClass::Type,
        ResolverClass::IndirectCall,
    ];

    /// Whether the analyzer resolves this class. Macro, include,
    /// behaviour, record, local call, and indirect call resolve on the
    /// target tree; qualified call and type resolve through the
    /// snapshot and self-pin.
    // exhaustive so a newly added class fails to compile here rather than silently return true
    #[must_use]
    pub fn is_covered(self) -> bool {
        match self {
            ResolverClass::Macro
            | ResolverClass::Include
            | ResolverClass::Behaviour
            | ResolverClass::QualifiedCall
            | ResolverClass::Record
            | ResolverClass::LocalCall
            | ResolverClass::Type
            | ResolverClass::IndirectCall => true,
        }
    }

    /// The class a symbol falls in. One class per kind, so the producer
    /// and the consumer agree without a hand-copied table: a function is
    /// a qualified call, a type its own class, a callback its behaviour.
    #[must_use]
    pub fn of_symbol_kind(kind: &SymbolKind) -> ResolverClass {
        match kind {
            SymbolKind::Macro { .. } => ResolverClass::Macro,
            SymbolKind::Record { .. } => ResolverClass::Record,
            SymbolKind::Behaviour { .. } | SymbolKind::Callback { .. } => ResolverClass::Behaviour,
            SymbolKind::Type { .. } => ResolverClass::Type,
            SymbolKind::Function { .. } | SymbolKind::FunctionAnyArity { .. } => {
                ResolverClass::QualifiedCall
            }
        }
    }
}

/// The set of classes a producer checks, stamped on a verdict envelope
/// so a corpus row self-describes its producer's coverage. Stores only
/// the checked set: `unchecked` is derived, so a class can never be in
/// both or neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ResolverCoverage {
    checked: BTreeSet<ResolverClass>,
}

impl ResolverCoverage {
    /// Coverage of the running binary.
    #[must_use]
    pub fn current() -> Self {
        Self {
            checked: ResolverClass::ALL
                .into_iter()
                .filter(|c| c.is_covered())
                .collect(),
        }
    }

    /// Whether this producer checked `class`: the fold's bug-vs-gap test.
    #[must_use]
    pub fn is_checked(&self, class: ResolverClass) -> bool {
        self.checked.contains(&class)
    }

    /// Classes this producer did not check: the calibration sampler steers
    /// toward candidates touching one.
    pub fn unchecked(&self) -> impl Iterator<Item = ResolverClass> + '_ {
        ResolverClass::ALL
            .into_iter()
            .filter(|c| !self.checked.contains(c))
    }
}
