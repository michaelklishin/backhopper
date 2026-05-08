//! Elixir source surface extractor for backhopper.
//!
//! Phase 4 placeholder: real `defmodule`/`def`/`@callback`/`@spec` extraction
//! lands once the Erlang extractor and CLI ship. This crate exists in the
//! workspace from day one so the dependency graph is stable.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElixirExtractor;

impl ElixirExtractor {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ElixirExtractor {
    fn default() -> Self {
        Self::new()
    }
}
