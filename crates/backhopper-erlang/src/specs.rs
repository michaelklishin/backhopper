// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-spec` / `-callback` / `-type` body parsing.

use backhopper_erlang_scan::{count_top_level_commas, take_balanced_parens};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignature {
    pub name: String,
    pub arity: u8,
    pub signature: String,
}

pub fn parse_callable_signature(body: &str) -> Option<ParsedSignature> {
    let trimmed = body.trim();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_string();
    let after_name = trimmed[name_end..].trim_start();
    if !after_name.starts_with('(') {
        return None;
    }
    let (args, rest_after_args) = take_balanced_parens(after_name)?;
    let arity = count_top_level_commas(args) + if args.trim().is_empty() { 0 } else { 1 };
    let rest = rest_after_args.trim_start();
    let after_arrow = rest.strip_prefix("->")?.trim_start();
    let signature = format!("{}({}) -> {}", name, args.trim(), after_arrow.trim());
    Some(ParsedSignature {
        name,
        arity: arity.min(255) as u8,
        signature,
    })
}

pub fn parse_type_decl(body: &str) -> Option<(String, u8, String)> {
    let trimmed = body.trim();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_string();
    let after_name = trimmed[name_end..].trim_start();
    if !after_name.starts_with('(') {
        return None;
    }
    let (args, rest_after_args) = take_balanced_parens(after_name)?;
    let arity = count_top_level_commas(args) + if args.trim().is_empty() { 0 } else { 1 };
    let rest = rest_after_args.trim_start();
    let after_op = rest.strip_prefix("::")?.trim_start();
    Some((name, arity.min(255) as u8, after_op.trim().to_string()))
}
