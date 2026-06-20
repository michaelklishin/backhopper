// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! What the intra-repo symbol resolver checks against the target tree.
//!
//! Declared once, not per verdict: the eval fold routes a build break
//! by class: a break in a checked class is a resolver bug, a break in
//! an unchecked class is a coverage gap. A static manifest lets the
//! fold make that call without a per-verdict evidence payload.

use serde::{Deserialize, Serialize};

/// A class of intra-repo symbol a backport can dangle on the target.
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
}

impl ResolverClass {
    pub const ALL: [ResolverClass; 6] = [
        ResolverClass::Macro,
        ResolverClass::Include,
        ResolverClass::Behaviour,
        ResolverClass::QualifiedCall,
        ResolverClass::Record,
        ResolverClass::LocalCall,
    ];

    /// Whether the analyzer resolves this class against the target.
    /// Macro, include, behaviour, record, and local call resolve on the
    /// target tree; `QualifiedCall` resolves through the snapshot and
    /// self-pin.
    #[must_use]
    pub fn is_covered(self) -> bool {
        match self {
            ResolverClass::Macro
            | ResolverClass::Include
            | ResolverClass::Behaviour
            | ResolverClass::QualifiedCall
            | ResolverClass::Record
            | ResolverClass::LocalCall => true,
        }
    }
}

/// The current resolver coverage, every class with its status.
#[must_use]
pub fn coverage() -> Vec<(ResolverClass, bool)> {
    ResolverClass::ALL
        .into_iter()
        .map(|c| (c, c.is_covered()))
        .collect()
}
