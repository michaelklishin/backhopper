// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A header's `-define`: the macro's identity and what it expands to.

use std::fmt;
use std::str::FromStr;

use backhopper_erlang_scan::{count_top_level_commas, take_balanced_parens};
use serde::{Deserialize, Serialize};

use crate::errors::NameError;
use crate::model::names::{Arity, MacroName};
use crate::model::normalised::NormalisedMacroBody;

/// Erlang treats `NAME` and `NAME(Args)` as different macros, and macros of
/// different arities as different macros, so the arity is part of the key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MacroKey {
    pub name: MacroName,
    pub arity: Option<Arity>,
}

impl MacroKey {
    #[must_use]
    pub fn object_like(name: MacroName) -> Self {
        Self { name, arity: None }
    }

    #[must_use]
    pub fn function_like(name: MacroName, arity: Arity) -> Self {
        Self {
            name,
            arity: Some(arity),
        }
    }

    #[must_use]
    pub fn parse_define(body: &str) -> Option<(Self, &str)> {
        let trimmed = body.trim_start();
        let name_end = trimmed
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '@'))
            .map_or(trimmed.len(), |(i, _)| i);
        let name = MacroName::new(&trimmed[..name_end]).ok()?;
        let rest = trimmed[name_end..].trim_start();
        let (arity, after) = if rest.starts_with('(') {
            let (args, after_args) = take_balanced_parens(rest)?;
            let count = if args.trim().is_empty() {
                0
            } else {
                count_top_level_commas(args) + 1
            };
            (Some(Arity::try_from(count).ok()?), after_args.trim_start())
        } else {
            (None, rest)
        };
        let value = after.strip_prefix(',')?;
        Some((Self { name, arity }, value))
    }
}

impl fmt::Display for MacroKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.arity {
            Some(arity) => write!(f, "{}/{arity}", self.name),
            None => write!(f, "{}", self.name),
        }
    }
}

impl FromStr for MacroKey {
    type Err = NameError;

    fn from_str(s: &str) -> Result<Self, NameError> {
        match s.split_once('/') {
            Some((name, arity)) => Ok(Self::function_like(name.parse()?, arity.parse()?)),
            None => Ok(Self::object_like(s.parse()?)),
        }
    }
}

/// `Conditional` is a macro defined under any `-ifdef`, `-ifndef` or `-if`
/// other than a `TEST` guard: no environment here could say which branch is
/// live, so no body is recorded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MacroBody {
    Text(NormalisedMacroBody),
    Conditional,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacroDecl {
    pub key: MacroKey,
    pub body: MacroBody,
}

impl MacroDecl {
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        if self.body == other.body {
            return self;
        }
        Self {
            key: self.key,
            body: MacroBody::Conditional,
        }
    }
}
