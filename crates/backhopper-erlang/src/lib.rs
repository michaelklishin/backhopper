// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang source surface extractor.
//!
//! Reads Erlang source text and yields the model types from `backhopper-core`.
//! The parser is line-oriented and only recognizes the attributes that affect
//! API compatibility: see the design doc and the modules in this crate.

pub mod attributes;
pub mod cond_compile;
pub mod deprecated;
pub mod extractor;
pub mod macros;
pub mod records;
pub mod specs;
pub mod tokenizer;
pub mod visibility;

pub use extractor::{ErlangExtractor, ExtractError, ExtractedSource};

/// Semantic version of the extractor's output shape. Bump on any change that
/// could rewrite extraction results for the same input (new attribute support,
/// fixed parser bug, new clause-head capture, etc.). Snapshots embed this in
/// their header so `snapshots verify --all` can warn on drift.
pub const EXTRACTOR_VERSION: &str = "1";
