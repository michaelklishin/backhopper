// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Erlang type-expression parser. Recursive descent over `-spec` and
//! `-callback` return-type text. Lossy by design: unrecognised
//! constructs collapse to `SpecType::Unknown` so the comparator stays
//! conservative.

use std::str;

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
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '(' if !in_string && !in_atom_quote => depth_paren += 1,
            ')' if !in_string && !in_atom_quote => depth_paren -= 1,
            '{' if !in_string && !in_atom_quote => depth_brace += 1,
            '}' if !in_string && !in_atom_quote => depth_brace -= 1,
            '[' if !in_string && !in_atom_quote => depth_brack += 1,
            ']' if !in_string && !in_atom_quote => depth_brack -= 1,
            '-' if !in_string
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && bytes[i + 1] as char == '>' =>
            {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn strip_when_guard(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        let ch = bytes[i] as char;
        match ch {
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '(' if !in_string && !in_atom_quote => depth_paren += 1,
            ')' if !in_string && !in_atom_quote => depth_paren -= 1,
            '{' if !in_string && !in_atom_quote => depth_brace += 1,
            '}' if !in_string && !in_atom_quote => depth_brace -= 1,
            '[' if !in_string && !in_atom_quote => depth_brack += 1,
            ']' if !in_string && !in_atom_quote => depth_brack -= 1,
            'w' if !in_string
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0
                && &bytes[i..i + 4] == b"when"
                && is_word_boundary(bytes, i, i + 4) =>
            {
                return rest[..i].trim_end();
            }
            _ => {}
        }
        i += 1;
    }
    rest.trim()
}

fn is_word_boundary(bytes: &[u8], start: usize, end: usize) -> bool {
    let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
    let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
    before_ok && after_ok
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
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
            b'-' | b'0'..=b'9' => self.parse_integer(),
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
            Some(c) => is_ident_byte(c),
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
        let start = self.cursor;
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.cursor += 1;
            } else {
                break;
            }
        }
        let raw = &self.bytes[start..self.cursor];
        let s = str::from_utf8(raw).unwrap_or("");
        match s.parse::<i64>() {
            Ok(v) => SpecType::Integer { value: v },
            Err(_) => SpecType::Unknown,
        }
    }

    fn parse_var_or_unknown(&mut self, budget: usize) -> SpecType {
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
            if is_ident_byte(b) {
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
            | "nonempty_list"
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
