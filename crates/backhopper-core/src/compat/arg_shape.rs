// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Coarse classification of argument shapes at call sites and clause heads.
//!
//! A clause-head pattern is encoded as `Vec<ArgShape>`: one entry per
//! formal parameter. A call site's arguments are encoded the same way:
//! one entry per actual. Matching is per-position: each call argument
//! must `satisfies` the clause's pattern at the same position.
//!
//! `Unknown` is the safe escape hatch: when the extractor cannot
//! classify an actual or pattern with confidence, it emits `Unknown`,
//! and matching always accepts `Unknown` on either side so no false
//! positives result.

use serde::{Deserialize, Serialize};

use crate::model::names::RecordName;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArgShape {
    /// Variable identifier (starts with `_` or capital letter).
    Variable,
    /// Atom literal: `foo`, `'quoted atom'`.
    Atom { name: String },
    /// Integer literal.
    Integer,
    /// Float literal.
    Float,
    /// Binary literal: `<<...>>`.
    Binary,
    /// List literal: `[...]`.
    List,
    /// Tuple literal: `{...}` (size is the number of elements).
    Tuple { size: usize },
    /// Record reference: `#name{...}`.
    Record { name: RecordName },
    /// String literal: `"..."`.
    String,
    /// Anonymous fun: `fun ... end` or `fun mod:f/A`.
    Fun,
    /// Anything the extractor cannot classify. Always matches.
    Unknown,
}

impl ArgShape {
    /// Returns true when `self` (a call-site argument) is acceptable
    /// against `pattern` (a clause-head argument at the pin).
    ///
    /// Conservative: we only return `false` when we can *prove* a
    /// mismatch from static information. Anything we can't classify
    /// confidently is treated as a potential match.
    ///
    /// `Unknown` on either side accepts anything. `Variable` on either
    /// side accepts anything too: a call-site variable can hold any
    /// runtime value, and a clause-head variable binds to any input.
    pub fn satisfies(&self, pattern: &ArgShape) -> bool {
        if matches!(self, ArgShape::Unknown) || matches!(pattern, ArgShape::Unknown) {
            return true;
        }
        if matches!(self, ArgShape::Variable) || matches!(pattern, ArgShape::Variable) {
            return true;
        }
        match (self, pattern) {
            (ArgShape::Atom { name: a }, ArgShape::Atom { name: b }) => a == b,
            (ArgShape::Tuple { size: a }, ArgShape::Tuple { size: b }) => a == b,
            (ArgShape::Record { name: a }, ArgShape::Record { name: b }) => a == b,
            (ArgShape::Integer, ArgShape::Integer)
            | (ArgShape::Float, ArgShape::Float)
            | (ArgShape::Binary, ArgShape::Binary)
            | (ArgShape::List, ArgShape::List)
            | (ArgShape::String, ArgShape::String)
            | (ArgShape::Fun, ArgShape::Fun) => true,
            _ => false,
        }
    }
}

/// True when `args` (the call site's actuals) satisfies *any* of the
/// supplied clause-head patterns. Empty `clauses` returns true: with
/// no pattern data recorded, we can't penalize the call.
pub fn satisfies_any(args: &[ArgShape], clauses: &[Vec<ArgShape>]) -> bool {
    if clauses.is_empty() {
        return true;
    }
    clauses.iter().any(|clause| {
        clause.len() == args.len()
            && args
                .iter()
                .zip(clause.iter())
                .all(|(actual, pattern)| actual.satisfies(pattern))
    })
}