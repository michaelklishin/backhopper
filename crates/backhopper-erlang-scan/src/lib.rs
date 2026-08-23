// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang lexical scanning primitives.
//!
//! One home for the byte-level state machine the compatibility and
//! call-graph layers share: strings, quoted atoms, backslash escapes,
//! `$c` char literals, `%` comments, and `()` `[]` `{}` `<<>>` balancing.
//! No model types, so the scanners return only offsets and slices and
//! cannot construct a domain primitive.

use std::borrow::Cow;

/// The attribute keywords that declare a type name: their visibility
/// and equivalence rules differ, which does not bear on whether a
/// reference to the name resolves.
pub fn is_type_declaration_keyword(name: &str) -> bool {
    matches!(name, "type" | "opaque" | "nominal")
}

/// The attribute keywords whose bodies are type expressions, so a
/// `mod:t(...)` inside them is a type reference, not a call.
pub fn opens_type_ref_context(name: &str) -> bool {
    is_type_declaration_keyword(name) || matches!(name, "spec" | "callback")
}

/// Byte length of the `$`-led char literal starting at `at`, covering
/// `$x`, the single-char escapes, `$\^X`, `$\xHH`, `$\x{...}`, and one
/// to three octal digits. `$` consumes exactly one char-or-escape.
pub fn skip_char_literal_span(bytes: &[u8], at: usize) -> usize {
    debug_assert_eq!(bytes[at], b'$');
    let next = at + 1;
    if next >= bytes.len() {
        return 1;
    }
    if bytes[next] != b'\\' {
        return 2;
    }
    let esc = next + 1;
    if esc >= bytes.len() {
        return 2;
    }
    match bytes[esc] {
        b'^' => {
            if esc + 1 < bytes.len() {
                4
            } else {
                3
            }
        }
        b'x' if bytes.get(esc + 1) == Some(&b'{') => {
            let mut i = esc + 2;
            while i < bytes.len() && bytes[i] != b'}' {
                i += 1;
            }
            if i < bytes.len() { i + 1 - at } else { i - at }
        }
        b'x' => {
            let mut i = esc + 1;
            let stop = (esc + 3).min(bytes.len());
            while i < stop && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            i - at
        }
        b'0'..=b'7' => {
            let mut i = esc + 1;
            let stop = (esc + 3).min(bytes.len());
            while i < stop && (b'0'..=b'7').contains(&bytes[i]) {
                i += 1;
            }
            i - at
        }
        _ => 3,
    }
}

/// Byte length of the string literal starting at `at`: an ordinary
/// escape-aware `"` string, a triple-quoted string, or a `~` sigil
/// string. `None` when `at` starts none of them. An unterminated
/// ordinary string runs to the end of the input.
pub fn string_span(bytes: &[u8], at: usize) -> Option<usize> {
    match bytes.get(at)? {
        b'"' => {
            if let Some(span) = triple_quoted_span(bytes, at) {
                return Some(span);
            }
            Some(quoted_span(bytes, at, b'"'))
        }
        b'~' => sigil_span(bytes, at),
        _ => None,
    }
}

/// Byte length of the quoted atom starting at `at`, escape-aware;
/// quoted atoms legally span newlines. `None` when `at` is not `'`.
/// An unterminated atom runs to the end of the input.
pub fn quoted_atom_span(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'\'') {
        return None;
    }
    Some(quoted_span(bytes, at, b'\''))
}

/// Byte length of the string, quoted atom, or char literal opening at
/// `at`, or `None` when none of them does. A scan that walks bytes and
/// must not read structure out of literal contents steps over the span
/// this returns.
pub fn literal_span(bytes: &[u8], at: usize) -> Option<usize> {
    match *bytes.get(at)? {
        b'"' | b'~' => string_span(bytes, at),
        b'\'' => quoted_atom_span(bytes, at),
        b'$' => Some(skip_char_literal_span(bytes, at)),
        _ => None,
    }
}

fn quoted_span(bytes: &[u8], at: usize, quote: u8) -> usize {
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = (i + 2).min(bytes.len()),
            b if b == quote => return i + 1 - at,
            _ => i += 1,
        }
    }
    i - at
}

/// True when the byte at `i` is a `.` that ends a form: the next byte
/// is whitespace, a `%` comment opener, or the end of the input. This
/// is `erl_scan`'s rule.
pub fn dot_terminates(bytes: &[u8], i: usize) -> bool {
    if bytes.get(i) != Some(&b'.') {
        return false;
    }
    match bytes.get(i + 1) {
        None => true,
        Some(b) => b.is_ascii_whitespace() || *b == b'%',
    }
}

/// Digit value for `number_span`'s based forms: `0-9`, then `a-z` and
/// `A-Z` as 10 to 35. `None` for anything else.
fn based_digit_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'z' => Some(u32::from(b - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(b - b'A') + 10),
        _ => None,
    }
}

/// Consume digits valid under `base` starting at `from`, with `_`
/// allowed between digits. Returns the position after the run.
fn based_digit_run(bytes: &[u8], from: usize, base: u32) -> usize {
    let is_digit = |b: u8| based_digit_value(b).is_some_and(|v| v < base);
    let mut i = from;
    while i < bytes.len() {
        if is_digit(bytes[i]) {
            i += 1;
        } else if bytes[i] == b'_' && i > from && bytes.get(i + 1).copied().is_some_and(is_digit) {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// Byte length of the numeric literal starting at `at`, or `None` when
/// `at` is not an ASCII digit. Covers integers with `_` separators,
/// `base#digits` for bases 2 to 36, decimal floats with optional
/// signed exponents, and based floats (`base#int.frac` with an
/// optional `#e` signed exponent). A malformed tail ends the span at
/// the last valid position.
pub fn number_span(bytes: &[u8], at: usize) -> Option<usize> {
    if !bytes.get(at)?.is_ascii_digit() {
        return None;
    }
    let int_end = based_digit_run(bytes, at, 10);
    if bytes.get(int_end) == Some(&b'#') {
        let base: Option<u32> = std::str::from_utf8(&bytes[at..int_end])
            .ok()
            .and_then(|s| s.replace('_', "").parse().ok());
        let Some(base) = base.filter(|b| (2..=36).contains(b)) else {
            return Some(int_end - at);
        };
        let digits_end = based_digit_run(bytes, int_end + 1, base);
        if digits_end == int_end + 1 {
            return Some(int_end - at);
        }
        let mut end = digits_end;
        if bytes.get(end) == Some(&b'.')
            && bytes
                .get(end + 1)
                .copied()
                .is_some_and(|b| based_digit_value(b).is_some_and(|v| v < base))
        {
            end = based_digit_run(bytes, end + 1, base);
            if bytes.get(end) == Some(&b'#')
                && let Some(exp) = exponent_run(bytes, end + 1)
            {
                end = exp;
            }
        }
        return Some(end - at);
    }
    let mut end = int_end;
    if bytes.get(end) == Some(&b'.')
        && bytes
            .get(end + 1)
            .copied()
            .is_some_and(|b| b.is_ascii_digit())
    {
        end = based_digit_run(bytes, end + 1, 10);
        if let Some(exp) = exponent_run(bytes, end) {
            end = exp;
        }
    }
    Some(end - at)
}

/// Consume `e|E [+-] digits` starting at `from`; `None` when the shape
/// is not a complete exponent.
fn exponent_run(bytes: &[u8], from: usize) -> Option<usize> {
    if !matches!(bytes.get(from), Some(b'e') | Some(b'E')) {
        return None;
    }
    let mut i = from + 1;
    if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
        i += 1;
    }
    if !bytes.get(i).copied().is_some_and(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(based_digit_run(bytes, i, 10))
}

/// True when the `#` at `at` sits inside one numeric literal, such as
/// the base or exponent hash of `16#ff` or `16#fe.fe#e16`. The scan
/// walks back to the token start, crossing name bytes and `.` or `#`
/// flanked by name bytes; the guard fires only when that start is a
/// digit at a token boundary and the number's span covers the `#`.
pub fn hash_inside_number(bytes: &[u8], at: usize) -> bool {
    debug_assert_eq!(bytes.get(at), Some(&b'#'));
    let mut start = at;
    while start > 0 {
        let prev = bytes[start - 1];
        if is_name_byte(prev) {
            start -= 1;
        } else if (prev == b'.' || prev == b'#')
            && start >= 2
            && is_name_byte(bytes[start - 2])
            && bytes.get(start).copied().is_some_and(is_name_byte)
        {
            start -= 1;
        } else {
            break;
        }
    }
    if start == at || !bytes[start].is_ascii_digit() {
        return false;
    }
    number_span(bytes, start).is_some_and(|n| start + n > at)
}

/// Length of the run of `"` bytes starting at `at`, zero when `at` is
/// not a quote or is past the end.
fn quote_run_len(bytes: &[u8], at: usize) -> usize {
    let mut i = at;
    while i < bytes.len() && bytes[i] == b'"' {
        i += 1;
    }
    i - at
}

fn skip_horizontal_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    i
}

/// Byte length of the EEP-64 triple-quoted string starting at `at`, or
/// `None` when `at` does not start one. Total: any position that does
/// not begin a run of three or more `"` returns `None`.
///
/// The string ends at the next line whose first non-whitespace content
/// is a run of at least as many quotes as the opener, and the span
/// covers through the end of that run. Content in between is verbatim:
/// an `-export([...]).` line inside a documentation block is string
/// content, not an attribute. An opener with no closer runs to the end
/// of the input, which is what a real parse of that source concludes.
pub fn triple_quoted_span(bytes: &[u8], at: usize) -> Option<usize> {
    let open = quote_run_len(bytes, at);
    if open < 3 {
        return None;
    }
    let mut i = at + open;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        if i == bytes.len() {
            break;
        }
        i += 1;
        let content = skip_horizontal_ws(bytes, i);
        let close = quote_run_len(bytes, content);
        if close >= open {
            return Some(content + close - at);
        }
    }
    Some(bytes.len() - at)
}

/// The closing delimiter matching an EEP-66 sigil's opening one, or
/// `None` when the byte cannot open a sigil string.
fn sigil_closer(open: u8) -> Option<u8> {
    match open {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        b'<' => Some(b'>'),
        b'/' | b'|' | b'#' | b'`' | b'\'' | b'"' => Some(open),
        _ => None,
    }
}

/// Byte length of the EEP-66 sigil string (prefix, string, suffix)
/// starting at the `~` at `at`, or `None` when `at` does not start one.
/// Only the empty, `b`, and `s` prefixes process backslash escapes,
/// matching `erl_scan`; an unterminated sigil runs to end of input.
pub fn sigil_span(bytes: &[u8], at: usize) -> Option<usize> {
    if bytes.get(at) != Some(&b'~') {
        return None;
    }
    let mut i = at + 1;
    while i < bytes.len() && is_name_byte(bytes[i]) {
        i += 1;
    }
    let open = *bytes.get(i)?;
    let closer = sigil_closer(open)?;
    let prefix = &bytes[at + 1..i];
    let escaped = matches!(prefix, b"" | b"b" | b"s");
    if open == b'"'
        && let Some(span) = triple_quoted_span(bytes, i)
    {
        i += span;
    } else {
        i += 1;
        loop {
            let Some(&b) = bytes.get(i) else {
                return Some(bytes.len() - at);
            };
            i += 1;
            if b == b'\\' && escaped {
                i += 1;
            } else if b == closer {
                break;
            }
        }
    }
    while i < bytes.len() && is_name_byte(bytes[i]) {
        i += 1;
    }
    Some(i - at)
}

/// Bracketless block tracking: `case`, `if`, `begin`, `receive`, and
/// `try` open a region that one `end` closes, and the expression forms
/// of `fun` and `maybe` do too. A comma inside such a region separates
/// statements or clauses, never arguments.
#[derive(Debug, Clone, Copy, Default)]
pub struct BlockDepth(i32);

impl BlockDepth {
    pub fn in_block(self) -> bool {
        self.0 > 0
    }

    /// Consume the identifier starting at `at` and adjust the depth.
    /// Returns the bytes consumed; the caller guarantees `at` is a
    /// token start on a lowercase letter outside literals.
    pub fn observe(&mut self, bytes: &[u8], at: usize) -> usize {
        let mut j = at;
        while j < bytes.len() && is_name_byte(bytes[j]) {
            j += 1;
        }
        // a keyword after `#` is a native-record name, not a block token
        if preceded_by_hash(bytes, at) {
            return j - at;
        }
        match &bytes[at..j] {
            b"case" | b"if" | b"begin" | b"receive" | b"try" => self.0 += 1,
            b"end" => self.0 = (self.0 - 1).max(0),
            b"fun" if fun_opens_block(bytes, j) => self.0 += 1,
            b"maybe" if maybe_opens_block(bytes, j) => self.0 += 1,
            _ => {}
        }
        j - at
    }
}

fn preceded_by_hash(bytes: &[u8], at: usize) -> bool {
    let mut i = at;
    while i > 0 && (bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
        i -= 1;
    }
    i > 0 && bytes[i - 1] == b'#'
}

// words that continue an expression rather than begin one: after atom
// `maybe` they are the only legal word followers
const EXPRESSION_CONTINUERS: [&[u8]; 19] = [
    b"of", b"when", b"andalso", b"orelse", b"end", b"after", b"else", b"div", b"rem", b"band",
    b"bor", b"bxor", b"bsl", b"bsr", b"and", b"or", b"xor", b"not", b"bnot",
];

/// `maybe` is a block opener since OTP 27 and a legal atom before it
/// (and after `-feature(maybe_expr, disable)`). The gate fails closed:
/// it opens a block only when the follower can begin a block body and
/// cannot follow an atom; every uncertain case, hunk truncation
/// included, keeps the atom reading and today's contained miscount.
fn maybe_opens_block(bytes: &[u8], after_maybe: usize) -> bool {
    let k = skip_ws(bytes, after_maybe);
    let Some(&b) = bytes.get(k) else {
        return false;
    };
    if b == b'<' {
        return bytes.get(k + 1) == Some(&b'<');
    }
    match b {
        b'[' | b'{' | b'"' | b'\'' | b'$' | b'~' => true,
        _ if b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit() => true,
        _ if b.is_ascii_lowercase() => {
            let mut m = k;
            while m < bytes.len() && is_name_byte(bytes[m]) {
                m += 1;
            }
            !EXPRESSION_CONTINUERS.contains(&&bytes[k..m])
        }
        _ => false,
    }
}

pub fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

/// True when `s` is a bare atom needing no quotes: a lowercase start
/// followed by name bytes only.
pub fn is_bare_atom(s: &str) -> bool {
    let bytes = s.as_bytes();
    match bytes.first() {
        Some(b) if b.is_ascii_lowercase() => {}
        _ => return false,
    }
    bytes.iter().all(|&b| is_name_byte(b))
}

/// Invokes `visit` at each byte of `s` sitting at bracket depth zero,
/// outside strings (ordinary, triple-quoted, and sigil), quoted atoms,
/// `$c` char literals, and `%` comments. Depth counts `()` `[]` `{}`
/// and `<<`/`>>` pairs. A `true` return stops the walk and yields that
/// byte's offset; `None` means the walk reached the end.
pub fn for_each_top_level_byte(s: &str, mut visit: impl FnMut(usize) -> bool) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut depth_angle = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'~' => {
                if let Some(span) = string_span(bytes, i) {
                    i += span;
                    continue;
                }
            }
            b'\'' => {
                if let Some(span) = quoted_atom_span(bytes, i) {
                    i += span;
                    continue;
                }
            }
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'%' => {
                i += line_comment_span(bytes, i);
                continue;
            }
            _ => {}
        }
        match bytes[i] {
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                depth_angle += 1;
                i += 2;
                continue;
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') && depth_angle > 0 => {
                depth_angle -= 1;
                i += 2;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 0 && depth_angle == 0 => {
                if visit(i) {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `s` with every `%`-to-newline comment outside literals removed.
/// List entries keep the comments the splitter saw around them; parsing
/// an entry needs them gone.
pub fn remove_line_comments(s: &str) -> Cow<'_, str> {
    let bytes = s.as_bytes();
    let mut out: Option<String> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        let span = match bytes[i] {
            b'%' => {
                out.get_or_insert_with(|| s[..i].to_string());
                i += line_comment_span(bytes, i);
                continue;
            }
            b'"' | b'~' => string_span(bytes, i).unwrap_or(1),
            b'\'' => quoted_atom_span(bytes, i).unwrap_or(1),
            b'$' => skip_char_literal_span(bytes, i),
            _ => 1,
        };
        // a span may end inside a multi-byte char (`$ä` reads two bytes)
        let mut end = (i + span).min(bytes.len());
        while !s.is_char_boundary(end) {
            end += 1;
        }
        if let Some(acc) = out.as_mut() {
            acc.push_str(&s[i..end]);
        }
        i = end;
    }
    match out {
        Some(acc) => Cow::Owned(acc),
        None => Cow::Borrowed(s),
    }
}

/// Bytes from the `%` at `at` to the end of the line, newline excluded.
/// Attribute bodies reach the scanners with their comments in place, so
/// a `% maybe` note in an export list must never feed the block depth.
fn line_comment_span(bytes: &[u8], at: usize) -> usize {
    let mut i = at;
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i - at
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'%' {
            i += line_comment_span(bytes, i);
            continue;
        }
        return i;
    }
}

/// The expression forms `fun (...) ->`, `fun Name(...) ->`, and their
/// guarded first clauses open a block; `fun m:f/1`-style references
/// and the type forms `fun()` and `fun((...) -> ...)` do not. What
/// follows the balanced paren group decides: an expression clause
/// continues with `->` or a `when` guard, a type or reference does
/// not, because a type's own `->` sits inside the group.
fn fun_opens_block(bytes: &[u8], after_fun: usize) -> bool {
    let mut j = skip_ws(bytes, after_fun);
    if bytes
        .get(j)
        .is_some_and(|b| b.is_ascii_uppercase() || *b == b'_')
    {
        while j < bytes.len() && is_name_byte(bytes[j]) {
            j += 1;
        }
        j = skip_ws(bytes, j);
    }
    if bytes.get(j) != Some(&b'(') {
        return false;
    }
    let Some(close) = matching_bracket(bytes, j) else {
        return false;
    };
    let j = skip_ws(bytes, close + 1);
    bytes[j..].starts_with(b"->")
        || (bytes[j..].starts_with(b"when") && !bytes.get(j + 4).copied().is_some_and(is_name_byte))
}

/// Byte offset of the bracket closing the `(`, `[`, or `{` at `open`,
/// tracking nesting, strings, quoted atoms, and char literals. Only the
/// opener's own kind is counted, so a bracket of another kind inside the
/// group cannot close it early.
pub fn matching_bracket(bytes: &[u8], open: usize) -> Option<usize> {
    let opener = *bytes.get(open)?;
    let closer = match opener {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        _ => return None,
    };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut in_atom = false;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str || in_atom {
            match b {
                b'\\' => i += 1,
                b'"' if in_str => in_str = false,
                b'\'' if in_atom => in_atom = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                if let Some(span) = triple_quoted_span(bytes, i) {
                    i += span;
                    continue;
                }
                in_str = true;
            }
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'%' => {
                i += line_comment_span(bytes, i);
                continue;
            }
            b'~' => {
                if let Some(span) = sigil_span(bytes, i) {
                    i += span;
                    continue;
                }
            }
            _ if b == opener => depth += 1,
            _ if b == closer => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Result of scanning one call's argument list after the opening `(`.
/// `Unterminated` means the closing paren is not in the input, so any
/// arity derived from it would be a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedArgs<'a> {
    Terminated { args: Vec<&'a str>, consumed: usize },
    Unterminated { args: Vec<&'a str> },
}

/// Arity evidence for one extracted call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanArity {
    Exact(u8),
    Unterminated,
}

/// The one argument-list scanner. Tracks nested `()` `[]` `{}` and
/// `<<>>`, strings, quoted atoms, backslash escapes, and `$c` char
/// literals (so `$,` `$(` `$"` stay inert).
pub fn scan_top_level_args(after_open_paren: &str) -> ScannedArgs<'_> {
    let bytes = after_open_paren.as_bytes();
    let mut args: Vec<&str> = Vec::new();
    let mut depth = 1i32;
    let mut blocks = BlockDepth::default();
    let mut in_str = false;
    let mut in_atom = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str || in_atom {
            match c {
                b'\\' => i += 1,
                b'"' if in_str => in_str = false,
                b'\'' if in_atom => in_atom = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        if c.is_ascii_lowercase() && (i == 0 || !is_name_byte(bytes[i - 1])) {
            i += blocks.observe(bytes, i);
            continue;
        }
        match c {
            b'"' => {
                if let Some(span) = triple_quoted_span(bytes, i) {
                    i += span;
                    continue;
                }
                in_str = true;
            }
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'%' => {
                i += line_comment_span(bytes, i);
                continue;
            }
            b'~' => {
                if let Some(span) = sigil_span(bytes, i) {
                    i += span;
                    continue;
                }
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 1;
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') && depth > 1 => {
                depth -= 1;
                i += 1;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b']' | b'}' => depth -= 1,
            b')' => {
                depth -= 1;
                // a close at depth zero inside an unclosed block means a
                // missing `end`: no terminated read, so no guessed arity
                if depth == 0 && !blocks.in_block() {
                    let slice = &after_open_paren[start..i];
                    if !slice.trim().is_empty() || !args.is_empty() {
                        args.push(slice);
                    }
                    return ScannedArgs::Terminated {
                        args,
                        consumed: i + 1,
                    };
                }
            }
            b',' if depth == 1 && !blocks.in_block() => {
                args.push(&after_open_paren[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        let slice = &after_open_paren[start..];
        if !slice.trim().is_empty() {
            args.push(slice);
        }
    }
    ScannedArgs::Unterminated { args }
}

pub fn scan_arity(after_open_paren: &str) -> ScanArity {
    match scan_top_level_args(after_open_paren) {
        ScannedArgs::Terminated { args, .. } => {
            ScanArity::Exact(u8::try_from(args.len()).unwrap_or(u8::MAX))
        }
        ScannedArgs::Unterminated { .. } => ScanArity::Unterminated,
    }
}

/// Splits the source after an already-consumed opening `(` into
/// top-level argument slices. Lenient about a missing close paren:
/// callers that must distinguish use `scan_top_level_args`.
pub fn split_top_level_args(after_open_paren: &str) -> Vec<&str> {
    match scan_top_level_args(after_open_paren) {
        ScannedArgs::Terminated { args, .. } | ScannedArgs::Unterminated { args } => args,
    }
}

/// Number of top-level items in `s[open..close]`, e.g. tuple elements.
pub fn count_top_level_items(s: &str, open: char, close: char) -> usize {
    let body = match (s.find(open), s.rfind(close)) {
        (Some(a), Some(b)) if b > a => &s[a + 1..b],
        _ => return 0,
    };
    if body.trim().is_empty() {
        return 0;
    }
    match scan_top_level_args(body) {
        ScannedArgs::Terminated { args, .. } | ScannedArgs::Unterminated { args } => args.len(),
    }
}

/// Invoke `on_comma` at each byte index of a comma sitting at bracket
/// depth zero, outside strings, atoms, char literals, binaries, and
/// `end`-closed block expressions. The shared separator definition
/// used by both the splitter and the counter.
fn for_each_top_level_comma(s: &str, mut on_comma: impl FnMut(usize)) {
    let bytes = s.as_bytes();
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut depth_angle = 0i32;
    let mut blocks = BlockDepth::default();
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if prev_back {
            prev_back = false;
            i += 1;
            continue;
        }
        if !in_string && !in_atom_quote && ch == '$' {
            i += skip_char_literal_span(bytes, i);
            continue;
        }
        if !in_string && !in_atom_quote && ch == '%' {
            i += line_comment_span(bytes, i);
            continue;
        }
        if !in_string
            && !in_atom_quote
            && ch == '"'
            && let Some(span) = triple_quoted_span(bytes, i)
        {
            i += span;
            continue;
        }
        if !in_string
            && !in_atom_quote
            && ch == '~'
            && let Some(span) = sigil_span(bytes, i)
        {
            i += span;
            continue;
        }
        if !in_string
            && !in_atom_quote
            && ch.is_ascii_lowercase()
            && (i == 0 || !is_name_byte(bytes[i - 1]))
        {
            i += blocks.observe(bytes, i);
            continue;
        }
        if !in_string && !in_atom_quote && ch == '<' && bytes.get(i + 1) == Some(&b'<') {
            depth_angle += 1;
            i += 2;
            continue;
        }
        if !in_string && !in_atom_quote && ch == '>' && bytes.get(i + 1) == Some(&b'>') {
            depth_angle -= 1;
            i += 2;
            continue;
        }
        match ch {
            '\\' if in_string || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '(' if !in_string && !in_atom_quote => depth_paren += 1,
            ')' if !in_string && !in_atom_quote => depth_paren -= 1,
            '{' if !in_string && !in_atom_quote => depth_brace += 1,
            '}' if !in_string && !in_atom_quote => depth_brace -= 1,
            '[' if !in_string && !in_atom_quote => depth_brack += 1,
            ']' if !in_string && !in_atom_quote => depth_brack -= 1,
            ',' if !in_string
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && depth_angle == 0
                && !blocks.in_block() =>
            {
                on_comma(i);
            }
            _ => {}
        }
        i += 1;
    }
}

/// Split `s` at top-level commas, trimming each piece and dropping empty
/// ones. For a bracketed list's inner text with no closing paren to stop
/// at, such as a record field list or an attribute argument list.
pub fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    for_each_top_level_comma(s, |i| {
        out.push(s[start..i].trim());
        start = i + 1;
    });
    if start < s.len() {
        out.push(s[start..].trim());
    }
    out.into_iter().filter(|x| !x.is_empty()).collect()
}

/// Count top-level commas in `s`. Arity counters add one for a non-empty
/// argument list.
pub fn count_top_level_commas(s: &str) -> usize {
    let mut n = 0usize;
    for_each_top_level_comma(s, |_| n += 1);
    n
}

/// Result of scanning one list literal's elements after the opening
/// `[`. `ImproperTail` means a top-level `|`, so the element count is
/// not the list's length. `Unterminated` means the closing bracket is
/// not in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScannedList<'a> {
    Terminated {
        elements: Vec<&'a str>,
        consumed: usize,
    },
    ImproperTail,
    Unterminated,
}

/// The list-literal form of `scan_top_level_args`: splits the source
/// after an already-consumed opening `[` into top-level element slices,
/// with the same string, atom, char-literal, and binary handling.
pub fn scan_list_elements(after_open_bracket: &str) -> ScannedList<'_> {
    let bytes = after_open_bracket.as_bytes();
    let mut elements: Vec<&str> = Vec::new();
    let mut depth = 1i32;
    let mut in_str = false;
    let mut in_atom = false;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str || in_atom {
            match c {
                b'\\' => i += 1,
                b'"' if in_str => in_str = false,
                b'\'' if in_atom => in_atom = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => {
                if let Some(span) = triple_quoted_span(bytes, i) {
                    i += span;
                    continue;
                }
                in_str = true;
            }
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'%' => {
                i += line_comment_span(bytes, i);
                continue;
            }
            b'~' => {
                if let Some(span) = sigil_span(bytes, i) {
                    i += span;
                    continue;
                }
            }
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 1;
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') && depth > 1 => {
                depth -= 1;
                i += 1;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b'}' => depth -= 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    let slice = &after_open_bracket[start..i];
                    if !slice.trim().is_empty() || !elements.is_empty() {
                        elements.push(slice);
                    }
                    return ScannedList::Terminated {
                        elements,
                        consumed: i + 1,
                    };
                }
            }
            // || is a comprehension, not a cons tail, and makes the element count meaningless
            b'|' if depth == 1 => return ScannedList::ImproperTail,
            b',' if depth == 1 => {
                elements.push(&after_open_bracket[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    ScannedList::Unterminated
}

/// A `-spec` or `-callback` body reduced to its name, arity, and the
/// canonical `name(Args) -> Ret` signature string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignature {
    pub name: String,
    pub arity: u8,
    pub signature: String,
}

/// Splits a callable head `name(Args)Rest` into the name, the inner
/// argument text, and the trailing text after the closing `)`. Returns
/// `None` when the head does not start with an unqualified name followed
/// by `(`: module-qualified forms (`Mod:f(...)`) fail because the `:`
/// ends the name and the next character is not `(`.
/// Splits a leading Erlang name off the front of `s`, returning the name
/// and the remaining text (both trimmed). The name may be empty when `s`
/// does not start with a name character.
pub fn split_leading_name(s: &str) -> (&str, &str) {
    let trimmed = s.trim_start();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    (&trimmed[..name_end], trimmed[name_end..].trim_start())
}

pub fn split_name_and_args(body: &str) -> Option<(&str, &str, &str)> {
    let (name, after_name) = split_leading_name(body);
    if name.is_empty() || !after_name.starts_with('(') {
        return None;
    }
    let (args, rest) = take_balanced_parens(after_name)?;
    Some((name, args, rest.trim_start()))
}

/// Top-level argument count for the inner text between a call's parens.
pub fn arity_of_args(args: &str) -> usize {
    count_top_level_commas(args) + if args.trim().is_empty() { 0 } else { 1 }
}

/// Parses a `-spec` or `-callback` body (`name(Args) -> Ret`, no
/// leading attribute keyword, no trailing `.`). Module-qualified forms
/// return `None`.
pub fn parse_callable_signature(body: &str) -> Option<ParsedSignature> {
    let (name, args, rest) = split_name_and_args(body)?;
    let after_arrow = rest.strip_prefix("->")?.trim_start();
    let signature = format!("{}({}) -> {}", name, args.trim(), after_arrow.trim());
    Some(ParsedSignature {
        name: name.to_string(),
        arity: arity_of_args(args).min(255) as u8,
        signature,
    })
}

/// Given `s` starting at `(`, returns the inner source and the rest
/// after the matching `)`, tracking nested parens, strings, and atoms.
pub fn take_balanced_parens(s: &str) -> Option<(&str, &str)> {
    if !s.starts_with('(') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if !in_string && !in_atom_quote && ch == '$' {
            i += skip_char_literal_span(bytes, i);
            continue;
        }
        if !in_string && !in_atom_quote && ch == '%' {
            i += line_comment_span(bytes, i);
            continue;
        }
        if !in_string
            && !in_atom_quote
            && ch == '"'
            && let Some(span) = triple_quoted_span(bytes, i)
        {
            i += span;
            continue;
        }
        if !in_string
            && !in_atom_quote
            && ch == '~'
            && let Some(span) = sigil_span(bytes, i)
        {
            i += span;
            continue;
        }
        match ch {
            '\\' if in_string || in_atom_quote => {
                i += 2;
                continue;
            }
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '(' if !in_string && !in_atom_quote => depth += 1,
            ')' if !in_string && !in_atom_quote => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
