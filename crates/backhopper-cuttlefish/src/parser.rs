// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::error::Error;
use std::fmt;
use std::path::Path;

use crate::fragments::{CuttlefishFragment, FragmentKind};

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ParseError {}

/// Parse a Cuttlefish `.schema` source into the list of top-level
/// translation/validator/mapping tuples it contains. Comments (`%`),
/// strings, and chars are balanced past correctly; nested `{`/`}` inside
/// strings or chars do not affect the brace stack.
pub fn parse_schema(
    source: &str,
    source_path: &Path,
) -> Result<Vec<CuttlefishFragment>, ParseError> {
    let bytes = source.as_bytes();
    let mut fragments: Vec<CuttlefishFragment> = Vec::new();
    let mut i = 0usize;
    let mut depth: i32 = 0;
    let mut top_open: Option<usize> = None;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                i = skip_string(bytes, i + 1);
            }
            b'\'' => {
                i = skip_atom_quoted(bytes, i + 1);
            }
            b'$' if i + 1 < bytes.len() => {
                // character literal: skip the char and any `\X` escape
                i += 2;
                if bytes[i - 1] == b'\\' && i < bytes.len() {
                    i += 1;
                }
            }
            b'{' => {
                if depth == 0 {
                    top_open = Some(i);
                }
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0
                    && let Some(open) = top_open.take()
                {
                    let span = &source[open..i];
                    if let Some(frag) = classify_top_level(span, open, source, source_path) {
                        fragments.push(frag);
                    }
                }
                if depth < 0 {
                    return Err(ParseError {
                        message: format!("unbalanced `}}` near byte {i}"),
                    });
                }
            }
            _ => i += 1,
        }
    }
    Ok(fragments)
}

fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn skip_atom_quoted(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b'\'' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

fn classify_top_level(
    span: &str,
    open_byte: usize,
    full_source: &str,
    source_path: &Path,
) -> Option<CuttlefishFragment> {
    let head = leading_atom(span);
    let kind = match head.as_deref() {
        Some("translation") => FragmentKind::Translation,
        Some("validator") => FragmentKind::Validator,
        Some("mapping") => FragmentKind::Mapping,
        _ => return None,
    };
    let key = leading_string_after_atom(span);
    let start_line = line_of_byte(full_source, open_byte);
    let (erlang_body, body_start_line) = match kind {
        FragmentKind::Mapping => (None, start_line),
        FragmentKind::Translation | FragmentKind::Validator => match locate_fun_body(span) {
            Some((body, body_offset_in_span)) => {
                let line = line_of_byte(full_source, open_byte + body_offset_in_span);
                (Some(body), line)
            }
            None => (None, start_line),
        },
    };
    Some(CuttlefishFragment {
        kind,
        source_path: source_path.to_path_buf(),
        start_line,
        erlang_body,
        body_start_line,
        key,
    })
}

fn leading_atom(span: &str) -> Option<String> {
    let s = span.trim_start_matches('{').trim_start();
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_lowercase() {
        return None;
    }
    let mut out = String::from(first);
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '_' || c == '@' {
            out.push(c);
        } else {
            break;
        }
    }
    Some(out)
}

fn leading_string_after_atom(span: &str) -> Option<String> {
    let after_brace = span.find('{').map(|i| i + 1)?;
    let rest = &span[after_brace..];
    let after_atom = rest.find(',')?;
    let tail = rest[after_atom + 1..].trim_start();
    if !tail.starts_with('"') {
        return None;
    }
    let after_quote = &tail[1..];
    let end_quote = find_unescaped(after_quote, b'"')?;
    Some(after_quote[..end_quote].to_owned())
}

/// Locate the body of the outer `fun(...) -> ... end` of a Cuttlefish
/// translation or validator tuple. Erlang nests `case ... end`,
/// `if ... end`, `receive ... end`, `try ... end`, `begin ... end`,
/// and inner `fun(...) -> ... end` inside the body, so we walk forward
/// from the body's first byte balancing those openers against `end`.
/// Returns `(body_text, body_offset_in_span)`.
fn locate_fun_body(span: &str) -> Option<(String, usize)> {
    let outer_fun = find_outer_fun_keyword(span)?;
    let arrow = find_substring(span, "->", outer_fun)?;
    let body_start = arrow + 2;
    let end_of_end = balanced_end_offset(span, body_start)?;
    let body = span[body_start..end_of_end].trim().to_owned();
    Some((body, body_start))
}

/// Find the byte offset of the outermost `fun` keyword that opens an
/// anonymous-fun block. Cuttlefish wraps tuples as
/// `{translation, "k", fun(C) -> ... end}` so the relevant `fun` is the
/// first one whose next non-whitespace character is `(`.
fn find_outer_fun_keyword(span: &str) -> Option<usize> {
    let bytes = span.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            i = skip_string(bytes, i + 1);
            continue;
        }
        if c == b'\'' {
            i = skip_atom_quoted(bytes, i + 1);
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() {
            i += 2;
            if bytes[i - 1] == b'\\' && i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if is_keyword_at(bytes, i, b"fun") && next_non_ws_is(bytes, i + 3, b'(') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Walk forward from `start` balancing Erlang block openers vs `end`,
/// starting with depth = 1 (we are already inside the outer fun).
/// Returns the byte offset of the matching `end`.
fn balanced_end_offset(span: &str, start: usize) -> Option<usize> {
    let bytes = span.as_bytes();
    let mut depth: i32 = 1;
    let mut i = start;
    'outer: while i < bytes.len() {
        let c = bytes[i];
        // Skip strings and quoted atoms so block-keywords inside them
        // do not confuse the depth counter.
        if c == b'"' {
            i = skip_string(bytes, i + 1);
            continue;
        }
        if c == b'\'' {
            i = skip_atom_quoted(bytes, i + 1);
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'$' && i + 1 < bytes.len() {
            i += 2;
            if bytes[i - 1] == b'\\' && i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if is_keyword_at(bytes, i, b"end") {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += 3;
            continue;
        }
        for opener in [
            b"case".as_slice(),
            b"if".as_slice(),
            b"receive".as_slice(),
            b"try".as_slice(),
            b"begin".as_slice(),
        ] {
            if is_keyword_at(bytes, i, opener) {
                depth += 1;
                i += opener.len();
                continue 'outer;
            }
        }
        if is_keyword_at(bytes, i, b"fun") && next_non_ws_is(bytes, i + 3, b'(') {
            depth += 1;
            i += 3;
            continue;
        }
        i += 1;
    }
    None
}

fn is_keyword_at(bytes: &[u8], at: usize, kw: &[u8]) -> bool {
    if at + kw.len() > bytes.len() {
        return false;
    }
    if &bytes[at..at + kw.len()] != kw {
        return false;
    }
    if at > 0 && is_ident_byte(bytes[at - 1]) {
        return false;
    }
    if at + kw.len() < bytes.len() && is_ident_byte(bytes[at + kw.len()]) {
        return false;
    }
    true
}

fn next_non_ws_is(bytes: &[u8], from: usize, ch: u8) -> bool {
    let mut i = from;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            i += 1;
            continue;
        }
        return b == ch;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn find_unescaped(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b if b == target => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn find_substring(s: &str, needle: &str, start: usize) -> Option<usize> {
    s.get(start..)?.find(needle).map(|p| p + start)
}

fn line_of_byte(s: &str, byte_offset: usize) -> usize {
    let mut line: usize = 1;
    for (i, c) in s.bytes().enumerate() {
        if i >= byte_offset {
            break;
        }
        if c == b'\n' {
            line += 1;
        }
    }
    line
}
