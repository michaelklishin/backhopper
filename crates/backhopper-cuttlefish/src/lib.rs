// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Cuttlefish `.schema` extractor.
//!
//! `.schema` files contain `{translation, ..., fun(C) -> Body end}` and
//! `{validator, ..., fun(V) -> Body end}` tuples whose `Body` is real
//! Erlang referencing host-repo modules. This crate locates the fun
//! bodies and reports their source location so the Erlang extractor
//! can be invoked over the embedded fragment.
//!
//! Applied to the `.schema`, `.snippets`, and `.partial` files a patch
//! touches; the extracted references are folded into the evaluated patch.

pub mod extract;
pub mod fragments;
pub mod parser;

pub use extract::extract_references;
pub use fragments::{CuttlefishFragment, FragmentKind};
pub use parser::{ParseError, parse_schema};

/// Version of the cuttlefish extractor's output shape. Bumped when the
/// extracted reference set changes for the same input.
pub const EXTRACTOR_VERSION: &str = "1";
