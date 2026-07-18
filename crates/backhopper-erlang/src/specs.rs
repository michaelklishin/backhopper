// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-spec` / `-callback` / `-type` body parsing.

use backhopper_erlang_scan::{arity_of_args, split_name_and_args};

// Promoted to the leaf crate so `backhopper-core` can parse specs with
// the same grammar; re-exported here to keep existing paths stable.
pub use backhopper_erlang_scan::{ParsedSignature, parse_callable_signature};

pub fn parse_type_decl(body: &str) -> Option<(String, u8, String)> {
    let (name, args, rest) = split_name_and_args(body)?;
    let after_op = rest.strip_prefix("::")?.trim_start();
    Some((
        name.to_string(),
        arity_of_args(args).min(255) as u8,
        after_op.trim().to_string(),
    ))
}
