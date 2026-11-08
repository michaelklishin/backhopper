// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::model::names::{ApplicationName, ConfKey, PartialName, ValidatorName};

/// One Cuttlefish top-level tuple located in a `.schema` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuttlefishFragment {
    pub kind: FragmentKind,
    /// File the fragment was found in.
    pub source_path: PathBuf,
    /// 1-based line of the opening `{` of the top-level tuple.
    pub start_line: usize,
    /// Erlang body of the `fun(...) -> ... end`, or the right-hand side of
    /// the mapping value. `None` for `mapping` tuples (path-only signal).
    pub erlang_body: Option<String>,
    /// 1-based line of the first character of `erlang_body` within the file.
    pub body_start_line: usize,
    /// Optional key string (the 2nd tuple element, when it is a string literal).
    pub key: Option<String>,
    /// Third tuple element of a `mapping` fragment: the target Erlang
    /// key, e.g. `"rabbit.message_interceptors"`. `None` for
    /// translations, validators, and mappings whose third element is
    /// not a string literal.
    pub mapping_target: Option<String>,
    /// Names of the attributes in a mapping's fourth tuple element
    /// (e.g. `"datatype"`, `"default"`, `"alias"`). Empty when absent.
    pub attr_names: Vec<String>,
    /// 1-based line of the closing `}` of the top-level tuple. Paired
    /// with `start_line` to give the fragment's full span.
    pub end_line: usize,
    pub partial_include: Option<PartialInclude>,
    pub referenced_validators: Vec<ValidatorName>,
}

/// An `{include_partial, {App, Name}, Opts}` term: the partial it pulls
/// in and the bare keys its `exclude` and `overrides` options name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialInclude {
    pub app: ApplicationName,
    pub name: PartialName,
    pub exclude: BTreeSet<ConfKey>,
    pub overrides: BTreeSet<ConfKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    Translation,
    Validator,
    Mapping,
    IncludePartial,
}

impl FragmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Validator => "validator",
            Self::Mapping => "mapping",
            Self::IncludePartial => "include_partial",
        }
    }
}
