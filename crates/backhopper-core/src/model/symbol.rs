use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName, TypeName};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    pub symbol: SymbolRef,
    pub from_path: PathBuf,
    pub line: usize,
}
