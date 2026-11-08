// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::error::Error;
use std::fmt;
use std::path::Path;

use backhopper_erlang_scan::{BlockDepth, quoted_atom_span, skip_char_literal_span, string_span};

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::model::names::{ApplicationName, ConfKey, PartialName, ValidatorName};

use crate::fragments::{CuttlefishFragment, FragmentKind, PartialInclude};

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

/// Parse a Cuttlefish `.schema` or `.partial` source into the list of
/// top-level translation (including `partial_translation`), validator,
/// mapping, and `include_partial` tuples it contains. Comments
/// (`%`), strings, and chars are balanced past correctly; nested `{`
/// and `}` inside strings or chars do not affect the brace stack.
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
            b'"' | b'~' => {
                i += string_span(bytes, i).unwrap_or(1);
            }
            b'\'' => {
                i += quoted_atom_span(bytes, i).unwrap_or(1);
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
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
                    if let Some(frag) = classify_top_level(span, open, i - 1, source, source_path) {
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

fn classify_top_level(
    span: &str,
    open_byte: usize,
    end_byte: usize,
    full_source: &str,
    source_path: &Path,
) -> Option<CuttlefishFragment> {
    let head = leading_atom(span);
    let kind = match head.as_deref() {
        Some("translation" | "partial_translation") => FragmentKind::Translation,
        Some("validator") => FragmentKind::Validator,
        Some("mapping") => FragmentKind::Mapping,
        Some("include_partial") => FragmentKind::IncludePartial,
        _ => return None,
    };
    let key = leading_string_after_atom(span);
    let start_line = line_of_byte(full_source, open_byte);
    let end_line = line_of_byte(full_source, end_byte);
    let (erlang_body, body_start_line) = match kind {
        FragmentKind::Mapping | FragmentKind::IncludePartial => (None, start_line),
        FragmentKind::Translation | FragmentKind::Validator => match locate_fun_body(span) {
            Some((body, body_offset_in_span)) => {
                let line = line_of_byte(full_source, open_byte + body_offset_in_span);
                (Some(body), line)
            }
            None => (None, start_line),
        },
    };
    let (mapping_target, attr_names) = match kind {
        FragmentKind::Mapping => mapping_target_and_attrs(span),
        FragmentKind::Translation | FragmentKind::Validator | FragmentKind::IncludePartial => {
            (None, Vec::new())
        }
    };
    let partial_include = match kind {
        FragmentKind::IncludePartial => parse_partial_include(span),
        _ => None,
    };
    let referenced_validators = match kind {
        FragmentKind::Mapping => mapping_referenced_validators(span),
        _ => Vec::new(),
    };
    Some(CuttlefishFragment {
        kind,
        source_path: source_path.to_path_buf(),
        start_line,
        erlang_body,
        body_start_line,
        key,
        mapping_target,
        attr_names,
        end_line,
        partial_include,
        referenced_validators,
    })
}

fn tuple_elements(span: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (span.find('{'), span.rfind('}')) else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    split_top_level(&span[open + 1..close])
}

/// `{include_partial, {App, "name"}, [{exclude, [...]}, {overrides, [...]}]}`.
/// The partial name is a string or an atom; `overrides` entries are
/// `{"key", Options}` tuples or bare key strings.
fn parse_partial_include(span: &str) -> Option<PartialInclude> {
    let elements = tuple_elements(span);
    let target = tuple_elements(elements.get(1)?);
    let app = ApplicationName::from_str(target.first()?.trim()).ok()?;
    let name_text = target.get(1)?;
    let name = string_literal(name_text).or_else(|| leading_atom(name_text.trim()))?;
    let options = elements.get(2).map(String::as_str).unwrap_or_default();
    let mut exclude = BTreeSet::new();
    let mut overrides = BTreeSet::new();
    for option in list_items(options) {
        let parts = tuple_elements(&option);
        let keys = parts
            .get(1)
            .map(|list| list_items(list))
            .unwrap_or_default();
        let into = match leading_atom(option.trim()).as_deref() {
            Some("exclude") => &mut exclude,
            Some("overrides") => &mut overrides,
            _ => continue,
        };
        into.extend(keys.iter().filter_map(|k| conf_key_of(k)));
    }
    Some(PartialInclude {
        app,
        name: PartialName::from_str(&name).ok()?,
        exclude,
        overrides,
    })
}

fn conf_key_of(item: &str) -> Option<ConfKey> {
    let text = item.trim();
    let literal = if text.starts_with('{') {
        string_literal(tuple_elements(text).first()?)?
    } else {
        string_literal(text)?
    };
    ConfKey::from_str(&literal).ok()
}

fn list_items(elem: &str) -> Vec<String> {
    let t = elem.trim();
    let (Some(open), Some(close)) = (t.find('['), t.rfind(']')) else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    split_top_level(&t[open + 1..close])
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .collect()
}

/// Validator names a mapping's fourth element refers to: its
/// `{validators, [...]}` list, and any `{validator, Name}` inside a
/// `datatype` option.
fn mapping_referenced_validators(span: &str) -> Vec<ValidatorName> {
    let elements = tuple_elements(span);
    let mut out = Vec::new();
    for option in elements.get(3).map(|e| list_items(e)).unwrap_or_default() {
        let parts = tuple_elements(&option);
        let Some(value) = parts.get(1) else { continue };
        match leading_atom(option.trim()).as_deref() {
            Some("validators") => out.extend(
                list_items(value)
                    .iter()
                    .filter_map(|v| string_literal(v))
                    .filter_map(|v| ValidatorName::from_str(&v).ok()),
            ),
            Some("datatype") => out.extend(inline_validators(value)),
            _ => {}
        }
    }
    out
}

fn inline_validators(text: &str) -> Vec<ValidatorName> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("{validator") {
        let tail = &rest[at + "{validator".len()..];
        rest = tail;
        let Some(after_comma) = tail.trim_start().strip_prefix(',') else {
            continue;
        };
        let name = string_literal(after_comma).or_else(|| leading_atom(after_comma.trim_start()));
        out.extend(name.and_then(|n| ValidatorName::from_str(&n).ok()));
    }
    out
}

/// A mapping tuple's third element (the target Erlang key, when a
/// string literal) and the attribute names in its fourth element
/// (a list of `{name, ...}` tuples).
fn mapping_target_and_attrs(span: &str) -> (Option<String>, Vec<String>) {
    let Some(open) = span.find('{') else {
        return (None, Vec::new());
    };
    let Some(close) = span.rfind('}') else {
        return (None, Vec::new());
    };
    if close <= open {
        return (None, Vec::new());
    }
    let elements = split_top_level(&span[open + 1..close]);
    let mapping_target = elements.get(2).and_then(|e| string_literal(e));
    let attr_names = elements
        .get(3)
        .map(|e| list_of_tuple_head_atoms(e))
        .unwrap_or_default();
    (mapping_target, attr_names)
}

/// Split `s` on commas at bracket depth zero, with comments, strings,
/// atoms, and char literals skipped so a comma inside any of those
/// never splits. `s` is the content strictly between one pair of
/// enclosing delimiters (already stripped by the caller).
fn split_top_level(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut elements = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' | b'~' => {
                i += string_span(bytes, i).unwrap_or(1);
                continue;
            }
            b'\'' => {
                i += quoted_atom_span(bytes, i).unwrap_or(1);
                continue;
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b',' if depth == 0 => {
                elements.push(s[start..i].to_owned());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    elements.push(s[start..].to_owned());
    elements
}

/// A top-level string literal, or `None` when the trimmed element
/// does not open with `"`.
fn string_literal(elem: &str) -> Option<String> {
    let t = elem.trim();
    let rest = t.strip_prefix('"')?;
    let end = find_unescaped(rest, b'"')?;
    Some(rest[..end].to_owned())
}

/// The leading atom of every top-level `{name, ...}` tuple in a list
/// literal (`elem` is the fourth mapping element, `[{alias, ...}, ...]`).
fn list_of_tuple_head_atoms(elem: &str) -> Vec<String> {
    let t = elem.trim();
    let Some(open) = t.find('[') else {
        return Vec::new();
    };
    let Some(close) = t.rfind(']') else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    split_top_level(&t[open + 1..close])
        .into_iter()
        .filter_map(|e| leading_atom(e.trim()))
        .collect()
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
    let arrow = find_arrow_skipping_literals(span.as_bytes(), outer_fun)?;
    let body_start = arrow + 2;
    let end_of_end = balanced_end_offset(span, outer_fun)?;
    let raw = &span[body_start..end_of_end];
    // the offset must skip the whitespace trim() dropped, or a body starting on the next line reports the arrow's line
    let leading_ws = raw.len() - raw.trim_start().len();
    let body = raw.trim().to_owned();
    Some((body, body_start + leading_ws))
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
        if c == b'"' || c == b'~' {
            i += string_span(bytes, i).unwrap_or(1);
            continue;
        }
        if c == b'\'' {
            i += quoted_atom_span(bytes, i).unwrap_or(1);
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'$' {
            i += skip_char_literal_span(bytes, i);
            continue;
        }
        if is_keyword_at(bytes, i, b"fun") && next_non_ws_is(bytes, i + 3, b'(') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Byte offset of the first `->` at or after `start`, with string,
/// atom, char, and comment spans skipped so an arrow inside a literal
/// cannot match.
fn find_arrow_skipping_literals(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'"' | b'~' => i += string_span(bytes, i).unwrap_or(1),
            b'\'' => i += quoted_atom_span(bytes, i).unwrap_or(1),
            b'$' => i += skip_char_literal_span(bytes, i),
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'-' if bytes[i + 1] == b'>' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Walk forward from the outer `fun` keyword at `start`, tracking
/// block depth through the shared `BlockDepth`. Returns the byte
/// offset of the `end` matching the outer fun: the position where the
/// depth transitions back to zero.
fn balanced_end_offset(span: &str, start: usize) -> Option<usize> {
    let bytes = span.as_bytes();
    let mut blocks = BlockDepth::default();
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        // skip literals so block keywords inside them do not affect the depth
        if c == b'"' || c == b'~' {
            i += string_span(bytes, i).unwrap_or(1);
            continue;
        }
        if c == b'\'' {
            i += quoted_atom_span(bytes, i).unwrap_or(1);
            continue;
        }
        if c == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'$' {
            i += skip_char_literal_span(bytes, i);
            continue;
        }
        if c.is_ascii_lowercase() && (i == 0 || !is_ident_byte(bytes[i - 1])) {
            let was_in_block = blocks.in_block();
            let consumed = blocks.observe(bytes, i);
            if was_in_block && !blocks.in_block() {
                return Some(i);
            }
            i += consumed.max(1);
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
