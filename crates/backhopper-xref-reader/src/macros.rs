// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-define` table and macro-body expansion helpers.
//!
//! Re-exports from `backhopper_core::erlang_macros` so the same macro
//! representation is shared between the xref reader and the compat
//! analyzer.

pub use backhopper_core::erlang_macros::{
    MacroKey, MacroTable, expand_value_macro_to_atom, expand_value_macro_to_mf, parse_define,
};