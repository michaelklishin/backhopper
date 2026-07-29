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

/// Byte length of the `$`-led char literal starting at `at`, covering
/// `$x`, `$\n`, and `$\^X` control-char escapes.
pub fn skip_char_literal_span(bytes: &[u8], at: usize) -> usize {
    debug_assert_eq!(bytes[at], b'$');
    let next = at + 1;
    if next >= bytes.len() {
        return 1;
    }
    if bytes[next] == b'\\' {
        if next + 1 < bytes.len() && bytes[next + 1] == b'^' && next + 2 < bytes.len() {
            return 4;
        }
        if next + 1 < bytes.len() {
            return 3;
        }
        return 2;
    }
    2
}

/// Bracketless block tracking: `case`, `if`, `begin`, `receive`, and
/// `try` open a region that one `end` closes, and the expression forms
/// of `fun` do too. A comma inside such a region separates statements
/// or clauses, never arguments. `maybe` is deliberately absent: it is
/// not a reserved word, so a bare `maybe` atom is legal in any
/// argument position and would desync the depth.
#[derive(Debug, Clone, Copy, Default)]
struct BlockDepth(i32);

impl BlockDepth {
    fn in_block(self) -> bool {
        self.0 > 0
    }

    /// Consume the identifier starting at `at` and adjust the depth.
    /// Returns the bytes consumed; the caller guarantees `at` is a
    /// token start on a lowercase letter outside literals.
    fn observe(&mut self, bytes: &[u8], at: usize) -> usize {
        let mut j = at;
        while j < bytes.len() && is_name_byte(bytes[j]) {
            j += 1;
        }
        match &bytes[at..j] {
            b"case" | b"if" | b"begin" | b"receive" | b"try" => self.0 += 1,
            b"end" => self.0 = (self.0 - 1).max(0),
            b"fun" if fun_opens_block(bytes, j) => self.0 += 1,
            _ => {}
        }
        j - at
    }
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
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
    let Some(close) = matching_paren(bytes, j) else {
        return false;
    };
    let j = skip_ws(bytes, close + 1);
    bytes[j..].starts_with(b"->")
        || (bytes[j..].starts_with(b"when") && !bytes.get(j + 4).copied().is_some_and(is_name_byte))
}

/// Byte offset of the `)` matching the `(` at `open`, tracking nested
/// parens, strings, quoted atoms, and char literals.
fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
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
            b'"' => in_str = true,
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'(' => depth += 1,
            b')' => {
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
            b'"' => in_str = true,
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
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
            b'"' => in_str = true,
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
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
