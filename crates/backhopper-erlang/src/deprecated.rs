// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Parse the four real source forms of `-deprecated` into one canonical
//! `Deprecation` value.

use crate::tokenizer::split_top_level_commas;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDeprecation {
    pub function: Option<String>,
    pub arity: Option<u8>,
    pub any_arity: bool,
    pub reason: Option<String>,
    pub module_wide: bool,
}

pub fn parse_deprecation_attribute(body: &str) -> Vec<ParsedDeprecation> {
    let inside = body.trim();
    let inside = inside
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(inside)
        .trim();
    if inside == "module" {
        return vec![ParsedDeprecation {
            function: None,
            arity: None,
            any_arity: true,
            reason: None,
            module_wide: true,
        }];
    }
    let list_inside = inside
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .map(str::trim)
        .unwrap_or(inside);
    let mut out = Vec::new();
    for chunk in split_top_level_commas(list_inside) {
        if let Some(parsed) = parse_one(chunk) {
            out.push(parsed);
        }
    }
    out
}

fn parse_one(s: &str) -> Option<ParsedDeprecation> {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
        .unwrap_or(trimmed);
    let parts = split_top_level_commas(inner);
    if parts.is_empty() {
        return None;
    }
    let function = parts[0].to_string();
    if parts.len() == 1 {
        return Some(ParsedDeprecation {
            function: Some(function),
            arity: None,
            any_arity: true,
            reason: None,
            module_wide: false,
        });
    }
    let arity_part = parts[1].trim();
    let any_arity = arity_part == "'_'" || arity_part == "_";
    let arity = if any_arity {
        None
    } else {
        arity_part.parse::<u8>().ok()
    };
    let reason = if parts.len() >= 3 {
        Some(strip_atom_or_string(parts[2]))
    } else {
        None
    };
    Some(ParsedDeprecation {
        function: Some(function),
        arity,
        any_arity,
        reason,
        module_wide: false,
    })
}

fn strip_atom_or_string(s: &str) -> String {
    let t = s.trim();
    let inner = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| {
            t.strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .unwrap_or(t)
        });
    inner.to_string()
}
