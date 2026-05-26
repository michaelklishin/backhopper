// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    Translation,
    Validator,
    Mapping,
}

impl FragmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Validator => "validator",
            Self::Mapping => "mapping",
        }
    }
}
