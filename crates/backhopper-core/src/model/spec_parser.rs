// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang type-expression parser. Recursive descent over `-spec` and
//! `-callback` return-type text. Lossy by design: unrecognised
//! constructs collapse to `SpecType::Unknown` so the comparator stays
//! conservative.

use std::str;

use backhopper_erlang_scan::{
    for_each_top_level_byte, is_name_byte, number_span, skip_char_literal_span,
};

use crate::model::spec_ast::SpecType;

pub fn parse_signature_return(signature: &str) -> SpecType {
    let Some(idx) = find_top_level_arrow(signature) else {
        return SpecType::Unknown;
    };
    let rest = &signature[idx + 2..];
    let body = strip_when_guard(rest);
    parse(body)
}

pub fn parse(text: &str) -> SpecType {
    let mut p = Parser::new(text);
    let t = p.parse_union();
    p.skip_ws();
    t.canonicalise()
}

fn find_top_level_arrow(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    for_each_top_level_byte(s, |i| bytes[i] == b'-' && bytes.get(i + 1) == Some(&b'>'))
}

fn strip_when_guard(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    let found = for_each_top_level_byte(rest, |i| {
        bytes[i..].starts_with(b"when") && is_word_boundary(bytes, i, i + 4)
    });
    match found {
        Some(i) => rest[..i].trim_end(),
        None => rest.trim(),
    }
}

fn is_word_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_name_byte(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_name_byte(bytes[end]);
    before_ok && after_ok
}

struct Parser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

const RECURSION_BUDGET: usize = 32;

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            cursor: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.cursor < self.bytes.len() {
            let b = self.bytes[self.cursor];
            if b.is_ascii_whitespace() {
                self.cursor += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.cursor + offset).copied()
    }

    fn eat(&mut self, expected: u8) -> bool {
        self.skip_ws();
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn parse_union(&mut self) -> SpecType {
        self.parse_union_with_budget(RECURSION_BUDGET)
    }

    fn parse_union_with_budget(&mut self, budget: usize) -> SpecType {
        if budget == 0 {
            return SpecType::Unknown;
        }
        let first = self.parse_single(budget - 1);
        self.skip_ws();
        if self.peek() != Some(b'|') {
            return first;
        }
        let mut variants = vec![first];
        while self.eat(b'|') {
            let v = self.parse_single(budget - 1);
            variants.push(v);
        }
        SpecType::Union { variants }
    }

    fn parse_single(&mut self, budget: usize) -> SpecType {
        if budget == 0 {
            return SpecType::Unknown;
        }
        self.skip_ws();
        let Some(b) = self.peek() else {
            return SpecType::Unknown;
        };
        match b {
            b'{' => self.parse_tuple(budget),
            b'[' => self.parse_list(budget),
            b'#' => self.parse_hash(budget),
            b'\'' => self.parse_quoted_atom(),
            b'_' => self.parse_underscore(),
            b'-' | b'0'..=b'9' | b'$' => self.parse_integer(),
            b'<' if self.peek_at(1) == Some(b'<') => self.parse_binary(),
            b if b.is_ascii_uppercase() => self.parse_var_or_unknown(budget),
            b if b.is_ascii_lowercase() => self.parse_lowercase_form(budget),
            _ => {
                self.skip_unknown();
                SpecType::Unknown
            }
        }
    }

    fn parse_tuple(&mut self, budget: usize) -> SpecType {
        debug_assert_eq!(self.peek(), Some(b'{'));
        self.cursor += 1;
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.cursor += 1;
            return SpecType::Tuple { arity: 0 };
        }
        let mut elements: Vec<SpecType> = Vec::new();
        loop {
            let elem = self.parse_union_with_budget(budget - 1);
            elements.push(elem);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.cursor += 1;
                }
                Some(b'}') => {
                    self.cursor += 1;
                    break;
                }
                _ => {
                    self.skip_until(b'}');
                    return SpecType::Unknown;
                }
            }
        }
        let arity = elements.len().min(u8::MAX as usize) as u8;
        match elements.first() {
            Some(SpecType::Atom { name }) => SpecType::TaggedTuple {
                tag: name.clone(),
                arity,
            },
            _ => SpecType::Tuple { arity },
        }
    }

    fn parse_list(&mut self, budget: usize) -> SpecType {
        debug_assert_eq!(self.peek(), Some(b'['));
        self.cursor += 1;
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.cursor += 1;
            return SpecType::List {
                element: Box::new(SpecType::Unknown),
            };
        }
        let elem = self.parse_union_with_budget(budget - 1);
        self.skip_until(b']');
        SpecType::List {
            element: Box::new(elem),
        }
    }

    fn parse_hash(&mut self, budget: usize) -> SpecType {
        debug_assert_eq!(self.peek(), Some(b'#'));
        self.cursor += 1;
        self.skip_ws();
        match self.peek() {
            Some(b'{') => {
                self.cursor += 1;
                self.skip_until_matching(b'{', b'}');
                SpecType::Map
            }
            Some(b) if b.is_ascii_lowercase() || b == b'\'' => {
                let name = self.read_atom_name();
                self.skip_ws();
                if self.peek() == Some(b'{') {
                    self.cursor += 1;
                    self.skip_until_matching(b'{', b'}');
                }
                SpecType::Record { name }
            }
            _ => {
                let _ = budget;
                self.skip_unknown();
                SpecType::Unknown
            }
        }
    }

    fn parse_quoted_atom(&mut self) -> SpecType {
        let name = self.read_quoted_atom();
        if name.is_empty() {
            return SpecType::Unknown;
        }
        SpecType::Atom { name }
    }

    fn parse_underscore(&mut self) -> SpecType {
        let next = self.peek_at(1);
        let is_word = match next {
            Some(c) => is_name_byte(c),
            None => false,
        };
        if !is_word {
            self.cursor += 1;
            return SpecType::Var {
                name: "_".to_owned(),
            };
        }
        let name = self.read_identifier();
        SpecType::Var { name }
    }

    fn parse_integer(&mut self) -> SpecType {
        let Some(low) = self.read_integer_value() else {
            self.skip_unknown();
            return SpecType::Unknown;
        };
        let save = self.cursor;
        self.skip_ws();
        if self.peek() == Some(b'.') && self.peek_at(1) == Some(b'.') {
            self.cursor += 2;
            self.skip_ws();
            let Some(high) = self.read_integer_value() else {
                self.skip_unknown();
                return SpecType::Unknown;
            };
            return SpecType::Range { low, high };
        }
        self.cursor = save;
        SpecType::Integer { value: low }
    }

    /// Reads one signed integer literal: decimal with `_` separators,
    /// `base#digits`, or a `$c` char literal read as its code point.
    /// `None` when the token is not a whole integer, such as a float.
    fn read_integer_value(&mut self) -> Option<i64> {
        let negative = self.peek() == Some(b'-');
        if negative {
            self.cursor += 1;
        }
        if self.peek() == Some(b'$') {
            let span = skip_char_literal_span(self.bytes, self.cursor);
            let raw = str::from_utf8(&self.bytes[self.cursor..self.cursor + span]).ok()?;
            let value = char_literal_value(raw)?;
            self.cursor += span;
            return Some(if negative { -value } else { value });
        }
        let span = number_span(self.bytes, self.cursor)?;
        let raw = str::from_utf8(&self.bytes[self.cursor..self.cursor + span]).ok()?;
        let value = integer_literal_value(raw)?;
        self.cursor += span;
        Some(if negative { -value } else { value })
    }

    fn parse_binary(&mut self) -> SpecType {
        debug_assert_eq!(self.peek(), Some(b'<'));
        self.cursor += 2;
        let mut depth = 1i32;
        while self.cursor < self.bytes.len() {
            if self.peek() == Some(b'<') && self.peek_at(1) == Some(b'<') {
                depth += 1;
                self.cursor += 2;
            } else if self.peek() == Some(b'>') && self.peek_at(1) == Some(b'>') {
                depth -= 1;
                self.cursor += 2;
                if depth == 0 {
                    return SpecType::Binary;
                }
            } else {
                self.cursor += 1;
            }
        }
        SpecType::Binary
    }

    fn parse_var_or_unknown(&mut self, budget: usize) -> SpecType {
        let name = self.read_identifier();
        self.skip_ws();
        match self.peek() {
            // annotation: the name documents, the type after `::` is the shape
            Some(b':') if self.peek_at(1) == Some(b':') => {
                self.cursor += 2;
                self.parse_union_with_budget(budget)
            }
            Some(b':') => {
                self.cursor += 1;
                let sub = self.read_identifier();
                self.skip_ws();
                if self.peek() == Some(b'(') {
                    let arity = self.read_call_args(budget);
                    SpecType::TypeCall {
                        module: Some(name),
                        name: sub,
                        arity,
                    }
                } else {
                    SpecType::Unknown
                }
            }
            _ => SpecType::Var { name },
        }
    }

    fn parse_lowercase_form(&mut self, budget: usize) -> SpecType {
        if self.is_at_keyword(b"fun") {
            self.cursor += 3;
            self.skip_balanced(b'(', b')');
            return SpecType::Fun;
        }
        let name = self.read_identifier();
        self.skip_ws();
        match self.peek() {
            Some(b':') => {
                self.cursor += 1;
                let sub = self.read_identifier();
                self.skip_ws();
                if self.peek() == Some(b'(') {
                    let arity = self.read_call_args(budget);
                    SpecType::TypeCall {
                        module: Some(name),
                        name: sub,
                        arity,
                    }
                } else {
                    SpecType::Unknown
                }
            }
            Some(b'(') => {
                let arity = self.read_call_args(budget);
                if is_builtin_type(&name) {
                    SpecType::Builtin { name }
                } else {
                    SpecType::TypeCall {
                        module: None,
                        name,
                        arity,
                    }
                }
            }
            _ => SpecType::Atom { name },
        }
    }

    fn is_at_keyword(&self, word: &[u8]) -> bool {
        if self.cursor + word.len() > self.bytes.len() {
            return false;
        }
        if &self.bytes[self.cursor..self.cursor + word.len()] != word {
            return false;
        }
        is_word_boundary(self.bytes, self.cursor, self.cursor + word.len())
    }

    fn read_identifier(&mut self) -> String {
        let start = self.cursor;
        while let Some(b) = self.peek() {
            if is_name_byte(b) {
                self.cursor += 1;
            } else {
                break;
            }
        }
        str::from_utf8(&self.bytes[start..self.cursor])
            .unwrap_or("")
            .to_owned()
    }

    fn read_atom_name(&mut self) -> String {
        if self.peek() == Some(b'\'') {
            self.read_quoted_atom()
        } else {
            self.read_identifier()
        }
    }

    fn read_quoted_atom(&mut self) -> String {
        debug_assert_eq!(self.peek(), Some(b'\''));
        self.cursor += 1;
        let start = self.cursor;
        let mut prev_escape = false;
        while let Some(b) = self.peek() {
            if prev_escape {
                prev_escape = false;
                self.cursor += 1;
                continue;
            }
            if b == b'\\' {
                prev_escape = true;
                self.cursor += 1;
                continue;
            }
            if b == b'\'' {
                let raw = &self.bytes[start..self.cursor];
                self.cursor += 1;
                return str::from_utf8(raw).unwrap_or("").to_owned();
            }
            self.cursor += 1;
        }
        String::new()
    }

    fn read_call_args(&mut self, budget: usize) -> u8 {
        debug_assert_eq!(self.peek(), Some(b'('));
        self.cursor += 1;
        self.skip_ws();
        if self.peek() == Some(b')') {
            self.cursor += 1;
            return 0;
        }
        let mut count = 0u32;
        loop {
            let _ = self.parse_union_with_budget(budget.saturating_sub(1));
            count += 1;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.cursor += 1,
                Some(b')') => {
                    self.cursor += 1;
                    break;
                }
                _ => {
                    self.skip_until(b')');
                    break;
                }
            }
        }
        count.min(u32::from(u8::MAX)) as u8
    }

    fn skip_unknown(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b',' | b'|' | b')' | b']' | b'}') {
                break;
            }
            self.cursor += 1;
        }
    }

    fn skip_until(&mut self, target: u8) {
        let mut depth_paren = 0i32;
        let mut depth_brace = 0i32;
        let mut depth_brack = 0i32;
        while let Some(b) = self.peek() {
            match b {
                b'(' => depth_paren += 1,
                b')' => {
                    if depth_paren == 0 && target == b')' {
                        self.cursor += 1;
                        return;
                    }
                    depth_paren -= 1;
                }
                b'{' => depth_brace += 1,
                b'}' => {
                    if depth_brace == 0 && target == b'}' {
                        self.cursor += 1;
                        return;
                    }
                    depth_brace -= 1;
                }
                b'[' => depth_brack += 1,
                b']' => {
                    if depth_brack == 0 && target == b']' {
                        self.cursor += 1;
                        return;
                    }
                    depth_brack -= 1;
                }
                _ => {}
            }
            self.cursor += 1;
        }
    }

    fn skip_until_matching(&mut self, open: u8, close: u8) {
        let mut depth = 1i32;
        while let Some(b) = self.peek() {
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    self.cursor += 1;
                    return;
                }
            }
            self.cursor += 1;
        }
    }

    fn skip_balanced(&mut self, open: u8, close: u8) {
        self.skip_ws();
        if self.peek() != Some(open) {
            return;
        }
        self.cursor += 1;
        self.skip_until_matching(open, close);
    }
}

/// Value of an integer literal slice: `_` separators and `base#digits`
/// forms. `None` for floats and out-of-range values.
fn integer_literal_value(raw: &str) -> Option<i64> {
    let s = raw.replace('_', "");
    if s.contains('.') {
        return None;
    }
    match s.split_once('#') {
        Some((base, digits)) => {
            if digits.contains('#') {
                return None;
            }
            let base: u32 = base.parse().ok()?;
            if !(2..=36).contains(&base) {
                return None;
            }
            i64::from_str_radix(digits, base).ok()
        }
        None => s.parse().ok(),
    }
}

/// Code point of a `$c` char literal slice, escapes included. `None`
/// for a malformed or truncated escape.
fn char_literal_value(raw: &str) -> Option<i64> {
    let body = raw.strip_prefix('$')?;
    let mut chars = body.chars();
    let first = chars.next()?;
    if first != '\\' {
        return Some(i64::from(u32::from(first)));
    }
    let esc = chars.next()?;
    let value = match esc {
        'n' => 10,
        'r' => 13,
        't' => 9,
        'b' => 8,
        'f' => 12,
        'e' => 27,
        's' => 32,
        'd' => 127,
        'v' => 11,
        '^' => u32::from(chars.next()?) & 31,
        'x' => {
            let rest = chars.as_str();
            let hex = rest
                .strip_prefix('{')
                .and_then(|r| r.strip_suffix('}'))
                .unwrap_or(rest);
            u32::from_str_radix(hex, 16).ok()?
        }
        '0'..='7' => {
            let mut v = u32::from(esc) - u32::from('0');
            for c in chars {
                v = v * 8 + c.to_digit(8)?;
            }
            v
        }
        other => u32::from(other),
    };
    Some(i64::from(value))
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "any"
            | "arity"
            | "atom"
            | "binary"
            | "bitstring"
            | "boolean"
            | "byte"
            | "char"
            | "dynamic"
            | "float"
            | "function"
            | "identifier"
            | "integer"
            | "iodata"
            | "iolist"
            | "list"
            | "map"
            | "maybe_improper_list"
            | "mfa"
            | "module"
            | "neg_integer"
            | "nil"
            | "no_return"
            | "node"
            | "non_neg_integer"
            | "none"
            | "nonempty_binary"
            | "nonempty_bitstring"
            | "nonempty_improper_list"
            | "nonempty_list"
            | "nonempty_maybe_improper_list"
            | "nonempty_string"
            | "number"
            | "pid"
            | "port"
            | "pos_integer"
            | "reference"
            | "string"
            | "term"
            | "timeout"
            | "tuple"
    )
}
