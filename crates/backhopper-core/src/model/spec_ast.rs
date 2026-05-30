// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Structured shape for Erlang type expressions inside `-spec` and
//! `-callback` declarations. `signature: String` stays the source of
//! truth on disk; this AST is built on demand from that string by
//! `backhopper-erlang::spec_parser`. Comparators on top drive the
//! Stage 9 return-shape conformance check.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecType {
    /// `{tag, T1, ...}` with `tag` a literal atom in head position.
    TaggedTuple { tag: String, arity: u8 },
    /// `{T1, T2, ...}` with no literal head atom.
    Tuple { arity: u8 },
    /// `T1 | T2 | T3`. Variants are canonicalised: flattened, sorted,
    /// deduped.
    Union { variants: Vec<SpecType> },
    /// `foo`, `'bar baz'`.
    Atom { name: String },
    /// `atom()`, `integer()`, `binary()`, `pid()`, `ok`, `term()`, etc.
    /// Distinct from `Atom` because the trailing `()` marks it as a
    /// type-call to a built-in.
    Builtin { name: String },
    /// `module:type()` or `type()` (when the type isn't a built-in).
    TypeCall {
        module: Option<String>,
        name: String,
        arity: u8,
    },
    /// `[T]` or `[]`.
    List { element: Box<SpecType> },
    /// `#name{...}`. Field types are not retained.
    Record { name: String },
    /// `#{...}`. Entry types are not retained.
    Map,
    /// `fun(...) -> R`. Parameter and return types are not retained.
    Fun,
    /// Uppercase identifier in a type position. Usually a guard-bound var.
    Var { name: String },
    /// Numeric literal in a type position.
    Integer { value: i64 },
    /// Anything else: the parser could not classify the expression.
    Unknown,
}

impl SpecType {
    pub fn canonicalise(self) -> Self {
        match self {
            Self::Union { variants } => {
                let mut flat = Vec::new();
                flatten_union(variants, &mut flat);
                flat.sort_by(canonical_cmp);
                flat.dedup();
                if flat.len() == 1 {
                    flat.pop().unwrap()
                } else if flat.is_empty() {
                    Self::Unknown
                } else {
                    Self::Union { variants: flat }
                }
            }
            other => other,
        }
    }

    /// `true` when `self` *might* be observed at a call site whose
    /// pattern matches `expected`. Used by the return-shape comparator.
    /// Conservative: returns `true` whenever either side carries
    /// `Unknown`. Returns `false` only when a positive incompatibility
    /// can be proven from the shape alone.
    pub fn matches(&self, expected: &SpecType) -> bool {
        if matches!(self, Self::Unknown) || matches!(expected, Self::Unknown) {
            return true;
        }
        match (self, expected) {
            (Self::Union { variants }, _) => variants.iter().any(|v| v.matches(expected)),
            (_, Self::Union { variants }) => variants.iter().any(|v| self.matches(v)),
            (Self::TaggedTuple { tag: a, arity: aa }, Self::TaggedTuple { tag: b, arity: bb }) => {
                a == b && aa == bb
            }
            (Self::TaggedTuple { arity: a, .. }, Self::Tuple { arity: b })
            | (Self::Tuple { arity: a }, Self::TaggedTuple { arity: b, .. }) => a == b,
            (Self::Tuple { arity: a }, Self::Tuple { arity: b }) => a == b,
            (Self::Atom { name: a }, Self::Atom { name: b }) => a == b,
            (Self::Builtin { name: a }, Self::Builtin { name: b }) => a == b,
            (Self::Atom { name: a }, Self::Builtin { name: b })
            | (Self::Builtin { name: a }, Self::Atom { name: b }) => a == b,
            (Self::List { .. }, Self::List { .. }) => true,
            (Self::Record { name: a }, Self::Record { name: b }) => a == b,
            (Self::Map, Self::Map) => true,
            (Self::Fun, Self::Fun) => true,
            (Self::Integer { value: a }, Self::Integer { value: b }) => a == b,
            (Self::Integer { .. }, Self::Builtin { name }) => {
                matches!(name.as_str(), "integer" | "non_neg_integer" | "pos_integer")
            }
            (Self::Builtin { name }, Self::Integer { .. }) => {
                matches!(name.as_str(), "integer" | "non_neg_integer" | "pos_integer")
            }
            (
                Self::TypeCall {
                    name: a, arity: aa, ..
                },
                Self::TypeCall {
                    name: b, arity: bb, ..
                },
            ) => a == b && aa == bb,
            (Self::Var { .. }, _) | (_, Self::Var { .. }) => true,
            _ => false,
        }
    }
}

fn flatten_union(variants: Vec<SpecType>, out: &mut Vec<SpecType>) {
    for v in variants {
        if let SpecType::Union { variants: inner } = v {
            flatten_union(inner, out);
        } else {
            out.push(v);
        }
    }
}

fn canonical_cmp(a: &SpecType, b: &SpecType) -> Ordering {
    let order = |t: &SpecType| -> (u8, String) {
        match t {
            SpecType::Atom { name } => (0, name.clone()),
            SpecType::Builtin { name } => (1, name.clone()),
            SpecType::Integer { value } => (2, value.to_string()),
            SpecType::TaggedTuple { tag, arity } => (3, format!("{tag}/{arity}")),
            SpecType::Tuple { arity } => (4, arity.to_string()),
            SpecType::List { .. } => (5, String::new()),
            SpecType::Map => (6, String::new()),
            SpecType::Fun => (7, String::new()),
            SpecType::Record { name } => (8, name.clone()),
            SpecType::TypeCall {
                module,
                name,
                arity,
            } => (
                9,
                format!("{}:{}/{}", module.as_deref().unwrap_or(""), name, arity),
            ),
            SpecType::Var { name } => (10, name.clone()),
            SpecType::Union { .. } => (11, String::new()),
            SpecType::Unknown => (12, String::new()),
        }
    };
    let (ka, va) = order(a);
    let (kb, vb) = order(b);
    match ka.cmp(&kb) {
        Ordering::Equal => va.cmp(&vb),
        other => other,
    }
}
