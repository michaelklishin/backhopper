// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared surface for the per-language source extractors.
//!
//! The token-driven Erlang scanner (`backhopper-erlang`) and the
//! regex-driven Elixir scanner (`backhopper-elixir`) keep their own bodies,
//! but speak this common vocabulary: the decode error, the extracted-source
//! bag, and the visibility classifier.

use std::path::PathBuf;

use thiserror::Error;

use crate::model::names::ModuleName;
use crate::model::snapshot::{HrlFile, Module, Visibility};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSource {
    pub modules: Vec<Module>,
    pub headers: Vec<HrlFile>,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("invalid utf-8 in {path:?} at byte offset {offset}")]
    InvalidUtf8 { path: PathBuf, offset: usize },
}

/// Resolves a module's [`Visibility`] from its name, the config's public and
/// internal module lists, an out-of-band `hidden` hint (Erlang `%% @hidden`
/// or `-doc(hidden)`, Elixir `@moduledoc false`), and whether its only
/// exports are `-ifdef(TEST)`-guarded. An explicit internal listing wins; a
/// public listing overrides the `hidden` hint.
pub fn classify_visibility(
    module: &ModuleName,
    hidden: bool,
    test_only: bool,
    public_modules: &[String],
    internal_modules: &[String],
) -> Visibility {
    if internal_modules.iter().any(|n| n == module.as_str()) {
        return Visibility::Hidden;
    }
    if hidden && !public_modules.iter().any(|n| n == module.as_str()) {
        return Visibility::Hidden;
    }
    if test_only {
        return Visibility::TestOnly;
    }
    Visibility::Public
}
