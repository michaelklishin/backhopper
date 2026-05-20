// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Call-site, function-reference, and call-kind types.

use std::fmt;

use backhopper_core::{Arity, FunctionName, Mfa, ModuleName};
use serde::{Deserialize, Serialize};

use crate::loc::Loc;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FunctionSig {
    pub name: FunctionName,
    pub arity: Arity,
}

impl FunctionSig {
    pub fn new(name: FunctionName, arity: Arity) -> Self {
        Self { name, arity }
    }
}

impl fmt::Display for FunctionSig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.name, self.arity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "ref", rename_all = "snake_case")]
pub enum FunctionRef {
    Concrete(Mfa),
    UnresolvedModule {
        function: FunctionName,
        arity: Option<Arity>,
    },
    UnresolvedFunction {
        module: ModuleName,
        arity: Option<Arity>,
    },
    UnresolvedBoth {
        arity: Option<Arity>,
    },
}

impl FunctionRef {
    pub fn as_concrete(&self) -> Option<&Mfa> {
        if let FunctionRef::Concrete(mfa) = self {
            Some(mfa)
        } else {
            None
        }
    }

    pub fn is_unresolved(&self) -> bool {
        !matches!(self, FunctionRef::Concrete(_))
    }

    pub fn module(&self) -> Option<&ModuleName> {
        match self {
            FunctionRef::Concrete(m) => Some(&m.module),
            FunctionRef::UnresolvedFunction { module, .. } => Some(module),
            _ => None,
        }
    }

    pub fn arity(&self) -> Option<Arity> {
        match self {
            FunctionRef::Concrete(m) => Some(m.arity),
            FunctionRef::UnresolvedModule { arity, .. }
            | FunctionRef::UnresolvedFunction { arity, .. }
            | FunctionRef::UnresolvedBoth { arity } => *arity,
        }
    }
}

impl fmt::Display for FunctionRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FunctionRef::Concrete(mfa) => write!(f, "{}", mfa),
            FunctionRef::UnresolvedModule { function, arity } => {
                write!(f, "'$M_EXPR':{}/{}", function, fmt_arity(*arity))
            }
            FunctionRef::UnresolvedFunction { module, arity } => {
                write!(f, "{}:'$F_EXPR'/{}", module, fmt_arity(*arity))
            }
            FunctionRef::UnresolvedBoth { arity } => {
                write!(f, "'$M_EXPR':'$F_EXPR'/{}", fmt_arity(*arity))
            }
        }
    }
}

fn fmt_arity(a: Option<Arity>) -> String {
    match a {
        Some(a) => a.to_string(),
        None => "-1".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LocalFunctionRef {
    Concrete {
        function: FunctionName,
        arity: Arity,
    },
    Unresolved {
        arity: Option<Arity>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum CallTarget {
    External(FunctionRef),
    Local(LocalFunctionRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CallKind {
    Direct,
    Apply,
    Spawn,
    FunRef,
    FunBody,
    BehaviourDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnresolvedCallSite {
    pub caller: Mfa,
    pub partial: FunctionRef,
    pub kind: CallKind,
    pub loc: Loc,
}