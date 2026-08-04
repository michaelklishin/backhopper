// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-record(name, {fields})` parser.

use backhopper_erlang_scan::{
    quoted_atom_span, remove_line_comments, skip_char_literal_span, split_leading_name, string_span,
};

use crate::tokenizer::split_top_level_commas;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecord {
    pub name: String,
    pub fields: Vec<ParsedRecordField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecordField {
    pub name: String,
    pub type_repr: Option<String>,
}

pub fn parse_record(body: &str) -> Option<ParsedRecord> {
    let inside = body.trim();
    let inside = inside
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(inside)
        .trim();
    let parts = split_top_level_commas(inside);
    if parts.is_empty() {
        return None;
    }
    let name = remove_line_comments(parts[0]).trim().to_string();
    if name.is_empty() {
        return None;
    }
    let fields_text = parts[1..].join(",");
    let fields_inner = fields_text
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
        .unwrap_or("");
    let mut fields = Vec::new();
    for chunk in split_top_level_commas(fields_inner) {
        if let Some(field) = parse_record_field(chunk) {
            fields.push(field);
        }
    }
    Some(ParsedRecord { name, fields })
}

fn parse_record_field(s: &str) -> Option<ParsedRecordField> {
    let s = remove_line_comments(s);
    let (name, rest) = split_leading_name(&s);
    if name.is_empty() {
        return None;
    }
    let name = name.to_string();
    if rest.is_empty() {
        return Some(ParsedRecordField {
            name,
            type_repr: None,
        });
    }
    let (eq, cc) = top_level_marks(rest);
    let type_repr = match (eq, cc) {
        // f = Default :: Type
        (Some(e), Some(c)) if e < c => Some(rest[c + 2..].trim().to_string()),
        // f :: Type = Default
        (Some(e), Some(c)) => Some(rest[c + 2..e].trim().to_string()),
        (None, Some(c)) => Some(rest[c + 2..].trim().to_string()),
        (_, None) => None,
    };
    let type_repr = type_repr.filter(|t| !t.is_empty());
    Some(ParsedRecordField { name, type_repr })
}

/// Byte offsets of the first top-level default `=` and the first
/// top-level `::` in a record field's tail. Strings, quoted atoms,
/// char literals, brackets, and binaries are opaque; a `=` that is
/// part of a two-byte operator (`=>`, `==`, `=<`, `=:=`, `=/=`, `>=`,
/// `/=`) is not a default separator.
fn top_level_marks(s: &str) -> (Option<usize>, Option<usize>) {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut eq = None;
    let mut cc = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' || b == b'~' {
            if let Some(span) = string_span(bytes, i) {
                i += span;
                continue;
            }
        }
        match b {
            b'\'' => {
                i += quoted_atom_span(bytes, i).unwrap_or(1);
                continue;
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 2;
                continue;
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') && depth > 0 => {
                depth -= 1;
                i += 2;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b':' if bytes.get(i + 1) == Some(&b':') && depth == 0 => {
                if cc.is_none() {
                    cc = Some(i);
                }
                i += 2;
                continue;
            }
            b'=' if depth == 0 => {
                let next_is_op = matches!(
                    bytes.get(i + 1),
                    Some(b'>') | Some(b'=') | Some(b'<') | Some(b':') | Some(b'/')
                );
                let prev_is_op =
                    i > 0 && matches!(bytes[i - 1], b'=' | b'<' | b'>' | b'!' | b'/' | b':');
                if !next_is_op && !prev_is_op && eq.is_none() {
                    eq = Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    (eq, cc)
}
