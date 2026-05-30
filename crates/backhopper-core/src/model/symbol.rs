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
    /// Inside a function body, top-level expression, or anywhere that
    /// is not a `-spec`, `-callback`, `-type`, or `-opaque` attribute.
    Body,
    /// Inside a `-spec`, `-callback`, `-type`, or `-opaque` attribute,
    /// where `mod:ident(...)` is a type reference, not a function call.
    TypeAttribute,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolRef {
    #[serde(flatten)]
    pub kind: SymbolKind,
}

impl SymbolRef {
    pub fn function(mfa: Mfa) -> Self {
        Self {
            kind: SymbolKind::Function { mfa },
        }
    }

    pub fn record(name: RecordName) -> Self {
        Self {
            kind: SymbolKind::Record { name },
        }
    }

    pub fn macro_use(name: impl Into<String>) -> Self {
        Self {
            kind: SymbolKind::Macro { name: name.into() },
        }
    }

    pub fn type_ref(module: ModuleName, name: TypeName, arity: Arity) -> Self {
        Self {
            kind: SymbolKind::Type {
                module,
                name,
                arity,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    pub symbol: SymbolRef,
    pub from_path: PathBuf,
    pub line: usize,
}
