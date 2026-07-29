// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName, TypeName};

/// Where in the source a reference was extracted from. The same
/// textual `mod:ident(...)` shape means a function call in body
/// context and a type reference in type-attribute context, so the
/// extractor must carry the surrounding context with each symbol it
/// emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefContext {
    /// Inside a function body or top-level expression.
    Body,
    /// Inside a `-spec`, `-callback`, `-type`, or `-opaque` attribute,
    /// where `mod:ident(...)` is a type reference, not a function call.
    TypeAttribute,
    /// Inside any other attribute form (`-export`, `-define`,
    /// `-record`, ...), where an identifier is never a local call.
    OtherAttribute,
}

impl RefContext {
    /// True in either attribute region; a local `name(` there is a
    /// type name, an export entry, or a macro parameter, never a call
    /// or a clause head.
    #[must_use]
    pub fn is_attribute(self) -> bool {
        matches!(self, Self::TypeAttribute | Self::OtherAttribute)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SymbolKind {
    Behaviour {
        module: ModuleName,
    },
    Callback {
        module: ModuleName,
        function: FunctionName,
        arity: Arity,
    },
    Function {
        mfa: Mfa,
    },
    /// A function reference whose arity could not be determined from
    /// the source line (the call's closing paren sits on a later
    /// line). Resolves against a snapshot when *any* export carries
    /// the name; never produces an arity mismatch.
    FunctionAnyArity {
        module: ModuleName,
        function: FunctionName,
    },
    Macro {
        name: String,
    },
    Record {
        name: RecordName,
    },
    Type {
        module: ModuleName,
        name: TypeName,
        arity: Arity,
    },
}

/// Which side of the hunk a reference was extracted from. `Added`
/// lines are requirements the patch introduces; `Context` lines are
/// pre-existing facts about the target and never drive verdicts.
/// `Added` orders first so a kind-level dedup keeps the stronger
/// origin.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RefOrigin {
    #[default]
    Added,
    Context,
}

impl RefOrigin {
    // &self is required by serde's skip_serializing_if predicate shape
    #[allow(clippy::trivially_copy_pass_by_ref)]
    #[must_use]
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SymbolRef {
    #[serde(flatten)]
    pub kind: SymbolKind,
    #[serde(default, skip_serializing_if = "RefOrigin::is_added")]
    pub origin: RefOrigin,
}

impl SymbolRef {
    pub fn function(mfa: Mfa) -> Self {
        Self {
            kind: SymbolKind::Function { mfa },
            origin: RefOrigin::Added,
        }
    }

    pub fn function_any_arity(module: ModuleName, function: FunctionName) -> Self {
        Self {
            kind: SymbolKind::FunctionAnyArity { module, function },
            origin: RefOrigin::Added,
        }
    }

    pub fn record(name: RecordName) -> Self {
        Self {
            kind: SymbolKind::Record { name },
            origin: RefOrigin::Added,
        }
    }

    pub fn macro_use(name: impl Into<String>) -> Self {
        Self {
            kind: SymbolKind::Macro { name: name.into() },
            origin: RefOrigin::Added,
        }
    }

    pub fn type_ref(module: ModuleName, name: TypeName, arity: Arity) -> Self {
        Self {
            kind: SymbolKind::Type {
                module,
                name,
                arity,
            },
            origin: RefOrigin::Added,
        }
    }

    #[must_use]
    pub fn with_origin(mut self, origin: RefOrigin) -> Self {
        self.origin = origin;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Reference {
    pub symbol: SymbolRef,
    pub from_path: PathBuf,
    pub line: usize,
}
