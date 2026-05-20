// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang source to call-graph reader for `backhopper-xref`.
//!
//! Walks `.erl` source through a minimal tokenizer and produces
//! [`ModuleData`] records containing exports, imports, behaviours,
//! callbacks, and call sites (with span-precise locations).

pub mod application;
pub mod errors;
pub mod macros;
pub mod model;
pub mod reader;
pub mod scanner;
pub mod suite_matcher;

pub use application::{ApplicationAssignment, ProjectLayout};
pub use errors::{ReadError, ReadWarning};
pub use macros::{MacroKey, MacroTable, parse_define};
pub use model::{CallSite, ModuleData, ReadOutput};
pub use reader::SourceReader;
pub use suite_matcher::AstSuiteMatcher;
