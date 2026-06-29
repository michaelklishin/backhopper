// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Strip comments, handle quoted atoms and strings, and locate attribute bodies.
//!
//! For each line starting with `-NAME` at column 0, we collect everything up
//! to the terminating `.` (with bracket balancing) into the attribute body.
//! Attributes come in two shapes that this captures uniformly:
//!  * `-name(args).`  body = `(args)`
//!  * `-name body.`   body = `body`
//!
//! The lexical primitives (`split_top_level_commas`,
//! `skip_char_literal_span`) live in `backhopper-erlang-scan` and are
//! re-exported here; this module keeps the block-oriented attribute-body
//! reader that drives `attributes.rs`.

pub use backhopper_erlang_scan::{skip_char_literal_span, split_top_level_commas};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeBlock {
    pub line: usize,
    pub name: String,
    pub body: String,
}

pub fn iterate_attributes(source: &str) -> Vec<AttributeBlock> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut line = 1usize;
    let mut at_line_start = true;
    while cursor < bytes.len() {
        let ch = bytes[cursor] as char;
        if ch == '\n' {
            line += 1;
            cursor += 1;
            at_line_start = true;
            continue;
        }
        if ch == '%' {
            while cursor < bytes.len() && bytes[cursor] as char != '\n' {
                cursor += 1;
            }
            continue;
        }
        if at_line_start && ch == '-' {
            let attr_line = line;
            let mut p = cursor + 1;
            let name_start = p;
            while p < bytes.len() {
                let c = bytes[p] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    p += 1;
                } else {
                    break;
                }
            }
            let name = source[name_start..p].to_string();
            if name.is_empty() {
                cursor = p;
                at_line_start = false;
                continue;
            }
            while p < bytes.len() && (bytes[p] as char == ' ' || bytes[p] as char == '\t') {
                p += 1;
            }
            let body_start = p;
            let (end_after_dot, advanced_lines) = consume_until_terminating_dot(source, p);
            let body_end = body_end_excluding_dot(source, end_after_dot);
            let body = source[body_start..body_end].trim().to_string();
            out.push(AttributeBlock {
                line: attr_line,
                name,
                body,
            });
            line += advanced_lines;
            cursor = end_after_dot;
            at_line_start = false;
            continue;
        }
        if !ch.is_whitespace() {
            at_line_start = false;
        }
        cursor += 1;
    }
    out
}

fn consume_until_terminating_dot(source: &str, start: usize) -> (usize, usize) {
    let bytes = source.as_bytes();
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut p = start;
    let mut newlines = 0usize;
    while p < bytes.len() {
        let ch = bytes[p] as char;
        if prev_back {
            prev_back = false;
            p += 1;
            continue;
        }
        if !in_string && !in_atom_quote && ch == '$' {
            let span = skip_char_literal_span(bytes, p);
            let end = (p + span).min(bytes.len());
            #[allow(clippy::naive_bytecount)]
            let nls = bytes[p..end].iter().filter(|&&b| b == b'\n').count();
            newlines += nls;
            p += span;
            continue;
        }
        match ch {
            '\\' if in_string || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '%' if !in_string && !in_atom_quote => {
                while p < bytes.len() && bytes[p] as char != '\n' {
                    p += 1;
                }
                continue;
            }
            '(' if !in_string && !in_atom_quote => depth_paren += 1,
            ')' if !in_string && !in_atom_quote => depth_paren -= 1,
            '{' if !in_string && !in_atom_quote => depth_brace += 1,
            '}' if !in_string && !in_atom_quote => depth_brace -= 1,
            '[' if !in_string && !in_atom_quote => depth_brack += 1,
            ']' if !in_string && !in_atom_quote => depth_brack -= 1,
            '\n' => newlines += 1,
            '.' if !in_string
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0 =>
            {
                let after = p + 1;
                let next = bytes.get(after).map(|&b| b as char);
                let is_terminator = matches!(
                    next,
                    Some('\n') | Some(' ') | Some('\t') | Some('\0') | None
                );
                let prev = if p > 0 {
                    Some(bytes[p - 1] as char)
                } else {
                    None
                };
                let prev_is_alnum =
                    matches!(prev, Some(c) if c.is_ascii_alphanumeric() || c == '_');
                let next_is_alnum =
                    matches!(next, Some(c) if c.is_ascii_alphanumeric() || c == '_');
                if is_terminator && !next_is_alnum && !(prev_is_alnum && next == Some('.')) {
                    return (after, newlines);
                }
            }
            _ => {}
        }
        p += 1;
    }
    (p, newlines)
}

fn body_end_excluding_dot(source: &str, end_after_dot: usize) -> usize {
    if end_after_dot == 0 {
        return 0;
    }
    let bytes = source.as_bytes();
    let mut p = end_after_dot.saturating_sub(1);
    while p > 0 && (bytes[p] as char).is_whitespace() {
        p -= 1;
    }
    if p < bytes.len() && bytes[p] as char == '.' {
        p
    } else {
        end_after_dot
    }
}

pub fn strip_outer_parens(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('(') && trimmed.ends_with(')') {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner.trim();
    }
    trimmed
}

pub fn strip_outer_brackets(s: &str) -> &str {
    let trimmed = s.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner.trim();
    }
    trimmed
}
