// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Extract function-clause head patterns from Erlang source. For each
//! top-level `name(Args) -> ...` clause, records `(name, arity)` and
//! the per-position `ArgShape` of the arguments. Conservative: when a
//! pattern can't be classified, `ArgShape::Unknown` collapses the slot
//! so downstream `ClauseMismatch` checks stay sound.

use std::collections::BTreeMap;
use std::str::FromStr;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::compat::call_sites::{classify_arg, split_top_level_args};
use backhopper_core::model::names::{Arity, FunctionName};
use backhopper_core::model::snapshot::FunArity;

use backhopper_erlang_scan::{dot_terminates, quoted_atom_span, string_span};

use crate::tokenizer::skip_char_literal_span;

pub fn extract(source: &str) -> BTreeMap<FunArity, Vec<Vec<ArgShape>>> {
    let mut out: BTreeMap<FunArity, Vec<Vec<ArgShape>>> = BTreeMap::new();
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut at_line_start = true;
    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch == '\n' {
            cursor += 1;
            at_line_start = true;
            continue;
        }
        if ch == '%' {
            cursor = skip_to_eol(bytes, cursor);
            continue;
        }
        if !at_line_start {
            cursor += 1;
            continue;
        }
        // Non-identifier starts at column 0 are never function-clause heads.
        if ch.is_whitespace() || ch == '-' || ch == '%' {
            cursor += 1;
            if !ch.is_whitespace() {
                at_line_start = false;
            }
            continue;
        }
        let Some((name, name_end)) = read_identifier(bytes, cursor) else {
            cursor += 1;
            at_line_start = false;
            continue;
        };
        let after_name = skip_ws(bytes, name_end);
        if bytes.get(after_name) != Some(&b'(') {
            cursor = after_name;
            at_line_start = false;
            continue;
        }
        let (args_src, after_args) = match take_balanced(bytes, after_name) {
            Some(pair) => pair,
            None => {
                cursor = after_name + 1;
                at_line_start = false;
                continue;
            }
        };
        let after_when_or_arrow = skip_when_then_arrow(bytes, after_args);
        let Some(arrow_end) = after_when_or_arrow else {
            cursor = after_args;
            at_line_start = false;
            continue;
        };
        let Ok(fn_name) = FunctionName::from_str(&name) else {
            cursor = arrow_end;
            at_line_start = false;
            continue;
        };
        let args_text = std::str::from_utf8(&bytes[args_src.0..args_src.1]).unwrap_or("");
        let pieces = split_top_level_args(args_text);
        let arity_u8 = pieces.len().min(u8::MAX as usize) as u8;
        let key = FunArity {
            name: fn_name,
            arity: Arity::new(arity_u8),
        };
        let shapes: Vec<ArgShape> = pieces.iter().map(|p| classify_arg(p)).collect();
        out.entry(key).or_default().push(shapes);
        // skip the body so its inner ; does not split this clause prematurely
        cursor = skip_body(bytes, arrow_end);
        at_line_start = bytes.get(cursor).copied() == Some(b'\n');
    }
    out
}

fn read_identifier(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    let first = *bytes.get(start)?;
    if !(first.is_ascii_lowercase() || first == b'_') {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'@')
    {
        end += 1;
    }
    let s = std::str::from_utf8(&bytes[start..end]).ok()?.to_owned();
    Some((s, end))
}

fn skip_ws(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() {
        let b = bytes[cursor];
        if b == b' ' || b == b'\t' {
            cursor += 1;
        } else {
            break;
        }
    }
    cursor
}

fn skip_to_eol(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}

/// Read `(...)` returning the inner byte range and the cursor *after*
/// the closing paren. Tracks bracket depth, strings, atom quotes, and
/// `%` comments inside the args.
fn take_balanced(bytes: &[u8], open_at: usize) -> Option<((usize, usize), usize)> {
    debug_assert_eq!(bytes[open_at], b'(');
    let inner_start = open_at + 1;
    let mut depth_paren = 1i32;
    let mut cursor = inner_start;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' | b'~' => {
                cursor += string_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'\'' => {
                cursor += quoted_atom_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'%' => {
                cursor = skip_to_eol(bytes, cursor);
                continue;
            }
            // a $ char literal like $( or $" is data: consume the whole span
            // so it cannot unbalance brackets or toggle string state
            b'$' => {
                cursor += skip_char_literal_span(bytes, cursor);
                continue;
            }
            b'(' => depth_paren += 1,
            b')' => {
                depth_paren -= 1;
                if depth_paren == 0 {
                    return Some(((inner_start, cursor), cursor + 1));
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// Returns the cursor just after the `->` arrow that ends a clause
/// head, skipping any `when` guard. `None` when no arrow is found.
fn skip_when_then_arrow(bytes: &[u8], from: usize) -> Option<usize> {
    let mut cursor = skip_inline_ws_and_newlines(bytes, from);
    if cursor + 4 <= bytes.len() && &bytes[cursor..cursor + 4] == b"when" {
        let after_word = cursor + 4;
        if after_word == bytes.len() || !is_ident_byte(bytes[after_word]) {
            cursor = after_word;
            cursor = skip_to_arrow(bytes, cursor)?;
            return Some(cursor + 2);
        }
    }
    let cursor = skip_inline_ws_and_newlines(bytes, cursor);
    if cursor + 2 <= bytes.len() && &bytes[cursor..cursor + 2] == b"->" {
        return Some(cursor + 2);
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn skip_inline_ws_and_newlines(bytes: &[u8], mut cursor: usize) -> usize {
    while cursor < bytes.len() {
        let b = bytes[cursor];
        if b.is_ascii_whitespace() {
            cursor += 1;
        } else {
            break;
        }
    }
    cursor
}

fn skip_to_arrow(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    while cursor + 1 < bytes.len() {
        match bytes[cursor] {
            b'"' | b'~' => {
                cursor += string_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'\'' => {
                cursor += quoted_atom_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'%' => {
                cursor = skip_to_eol(bytes, cursor);
                continue;
            }
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'{' => depth_brace += 1,
            b'}' => depth_brace -= 1,
            b'[' => depth_brack += 1,
            b']' => depth_brack -= 1,
            // a $ char literal like $" or $- is data: consume the whole span
            // so it cannot toggle string state or read as the -> arrow
            b'$' => {
                cursor += skip_char_literal_span(bytes, cursor);
                continue;
            }
            b'-' if depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && bytes[cursor + 1] == b'>' =>
            {
                return Some(cursor);
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

/// Skip the body of a function clause, returning a cursor positioned
/// at the next top-level boundary (the next `\n` after the terminating
/// `.` or `;`).
fn skip_body(bytes: &[u8], from: usize) -> usize {
    let mut cursor = from;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' | b'~' => {
                cursor += string_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'\'' => {
                cursor += quoted_atom_span(bytes, cursor).unwrap_or(1);
                continue;
            }
            b'%' => {
                cursor = skip_to_eol(bytes, cursor);
                continue;
            }
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'{' => depth_brace += 1,
            b'}' => depth_brace -= 1,
            b'[' => depth_brack += 1,
            b']' => depth_brack -= 1,
            // a $ char literal like $" or $; is data: consume the whole span
            // so it cannot toggle string state or read as a clause terminator
            b'$' => {
                cursor += skip_char_literal_span(bytes, cursor);
                continue;
            }
            b'.' if depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && dot_terminates(bytes, cursor) =>
            {
                return cursor + 1;
            }
            b';' if depth_paren == 0 && depth_brace == 0 && depth_brack == 0 => {
                let next = bytes.get(cursor + 1).copied();
                if matches!(next, Some(b'\n') | Some(b' ') | Some(b'\t') | None) {
                    return cursor + 1;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    cursor
}
