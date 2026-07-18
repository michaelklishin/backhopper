// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanner for Erlang source.
//!
//! Tracks position (line, column, byte offset), skips comments and strings,
//! and exposes helpers the reader uses to find function clauses and call
//! sites. Not a full Erlang tokenizer: just what the v1 xref reader needs.
//!
//! A separate block-oriented tokenizer exists in `backhopper_erlang::tokenizer`.
//! Consolidation is deferred until a third consumer needs it.

use backhopper_xref_graph::Position;

#[derive(Debug, Clone)]
pub struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: Position,
}

impl<'a> Scanner<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: Position::zero(),
        }
    }

    pub fn pos(&self) -> Position {
        self.pos
    }

    pub fn src(&self) -> &'a str {
        self.src
    }

    pub fn src_slice(&self, start: usize, end: usize) -> &'a str {
        &self.src[start..end]
    }

    /// Set position. Used for backtracking when a heuristic match fails.
    pub fn set_pos(&mut self, pos: Position) {
        self.pos = pos;
    }

    pub fn is_eof(&self) -> bool {
        self.pos.byte_offset as usize >= self.bytes.len()
    }

    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos.byte_offset as usize).copied()
    }

    pub fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes
            .get(self.pos.byte_offset as usize + offset)
            .copied()
    }

    pub fn advance(&mut self) -> Option<u8> {
        let p = self.pos.byte_offset as usize;
        if p >= self.bytes.len() {
            return None;
        }
        let b = self.bytes[p];
        self.pos.byte_offset += 1;
        if b == b'\n' {
            self.pos.line += 1;
            self.pos.column = 1;
        } else {
            self.pos.column += 1;
        }
        Some(b)
    }

    pub fn skip_line(&mut self) {
        while let Some(b) = self.peek() {
            self.advance();
            if b == b'\n' {
                break;
            }
        }
    }

    /// Skip whitespace and Erlang `% ...` comments. Stops at the first
    /// non-trivia byte.
    pub fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_whitespace() => {
                    self.advance();
                }
                Some(b'%') => self.skip_line(),
                _ => break,
            }
        }
    }

    /// Consume a string literal starting at `"`, leaving the cursor
    /// just past the closing quote. Handles escaped characters minimally.
    pub fn consume_string(&mut self) {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.advance();
        while let Some(b) = self.peek() {
            self.advance();
            if b == b'\\' {
                self.advance();
                continue;
            }
            if b == b'"' {
                break;
            }
        }
    }

    /// Consume a quoted-atom literal starting at `'`, leaving the cursor
    /// just past the closing quote. Does not validate the contents.
    pub fn consume_quoted_atom(&mut self) {
        debug_assert_eq!(self.peek(), Some(b'\''));
        self.advance();
        while let Some(b) = self.peek() {
            self.advance();
            if b == b'\\' {
                self.advance();
                continue;
            }
            if b == b'\'' {
                break;
            }
        }
    }

    /// Consume a `$c`, `$\n`, or `$\^X` character literal.
    pub fn consume_char_literal(&mut self) {
        debug_assert_eq!(self.peek(), Some(b'$'));
        self.advance();
        if self.peek() == Some(b'\\') {
            self.advance();
            if self.peek() == Some(b'^') {
                self.advance();
            }
            self.advance();
        } else {
            self.advance();
        }
    }

    /// Read an unquoted identifier (atom or variable). Returns the slice
    /// of source covered.
    pub fn consume_identifier(&mut self) -> &'a str {
        let start = self.pos.byte_offset as usize;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'@' {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.pos.byte_offset as usize;
        &self.src[start..end]
    }

    /// Read a `?MACRO` reference returning the macro name without the `?`.
    pub fn consume_macro_ref(&mut self) -> &'a str {
        debug_assert_eq!(self.peek(), Some(b'?'));
        self.advance();
        let start = self.pos.byte_offset as usize;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.pos.byte_offset as usize;
        &self.src[start..end]
    }

    /// Skip a balanced parenthesized expression, beginning at `(`.
    /// Tracks nested `()`, `[]`, `{}`, strings, and atoms.
    pub fn skip_balanced_parens(&mut self) -> u32 {
        debug_assert_eq!(self.peek(), Some(b'('));
        self.advance();
        let mut depth: i32 = 1;
        let mut top_level_commas: u32 = 0;
        while let Some(b) = self.peek() {
            match b {
                b'(' => {
                    depth += 1;
                    self.advance();
                }
                b')' => {
                    depth -= 1;
                    self.advance();
                    if depth == 0 {
                        return top_level_commas;
                    }
                }
                b'[' | b'{' => {
                    // Inner brackets: track depth through them.
                    let opener = b;
                    let closer = if opener == b'[' { b']' } else { b'}' };
                    self.advance();
                    let mut inner = 1i32;
                    while let Some(b2) = self.peek() {
                        match b2 {
                            x if x == opener => {
                                inner += 1;
                                self.advance();
                            }
                            x if x == closer => {
                                inner -= 1;
                                self.advance();
                                if inner == 0 {
                                    break;
                                }
                            }
                            b'"' => self.consume_string(),
                            b'\'' => self.consume_quoted_atom(),
                            b'$' => self.consume_char_literal(),
                            b'%' => self.skip_line(),
                            _ => {
                                self.advance();
                            }
                        }
                    }
                }
                b',' if depth == 1 => {
                    top_level_commas += 1;
                    self.advance();
                }
                b'"' => self.consume_string(),
                b'\'' => self.consume_quoted_atom(),
                b'$' => self.consume_char_literal(),
                b'%' => self.skip_line(),
                _ => {
                    self.advance();
                }
            }
        }
        top_level_commas
    }
}

/// Outcome of a `walk_paren_group` pass over a `(...)` group.
#[derive(Debug, Clone, Copy)]
pub struct ParenWalk {
    pub commas: u32,
    pub had_content: bool,
    pub closed: bool,
}

/// Scans the balanced `(...)` group at the current `(`, which must be the
/// next byte. Tracks nested `()`, `[]`, `{}`, and `<<>>`; skips strings,
/// quoted atoms, char literals, and `%` comments. At each byte that can
/// begin an identifier or `?macro` use, `on_ident` runs with the cursor on
/// that byte so callers can surface nested call sites; if it returns false
/// the byte is consumed and the walk continues. Reports the top-level comma
/// count, whether any non-whitespace content appeared, and whether the
/// matching `)` was reached.
pub fn walk_paren_group(
    sc: &mut Scanner<'_>,
    mut on_ident: impl FnMut(&mut Scanner<'_>) -> bool,
) -> ParenWalk {
    debug_assert_eq!(sc.peek(), Some(b'('));
    sc.advance();
    let mut depth = 1i32;
    let mut commas: u32 = 0;
    let mut had_content = false;
    let mut closed = false;
    while let Some(byte) = sc.peek() {
        match byte {
            b'(' => {
                depth += 1;
                had_content = true;
                sc.advance();
            }
            b')' => {
                depth -= 1;
                sc.advance();
                if depth == 0 {
                    closed = true;
                    break;
                }
            }
            b'[' | b'{' => {
                depth += 1;
                had_content = true;
                sc.advance();
            }
            b']' | b'}' => {
                depth -= 1;
                sc.advance();
            }
            // << and >> nest like brackets so commas inside a binary literal are not counted as arguments.
            b'<' if sc.peek_at(1) == Some(b'<') => {
                depth += 1;
                had_content = true;
                sc.advance();
                sc.advance();
            }
            b'>' if sc.peek_at(1) == Some(b'>') => {
                depth -= 1;
                sc.advance();
                sc.advance();
            }
            b',' if depth == 1 => {
                commas += 1;
                sc.advance();
            }
            b'"' => {
                sc.consume_string();
                had_content = true;
            }
            b'\'' => {
                sc.consume_quoted_atom();
                had_content = true;
            }
            b'$' => {
                sc.consume_char_literal();
                had_content = true;
            }
            b'%' => sc.skip_line(),
            b'?' | b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                had_content = true;
                if !on_ident(sc) {
                    sc.advance();
                }
            }
            _ => {
                if !byte.is_ascii_whitespace() {
                    had_content = true;
                }
                sc.advance();
            }
        }
    }
    ParenWalk {
        commas,
        had_content,
        closed,
    }
}

/// Returns the arity inferred from the argument-list source `(a, b, c)`.
/// Empty parens yields 0; one or more top-level commas yields commas+1.
pub fn arity_from_commas(top_level_commas: u32, had_any_content: bool) -> u32 {
    if had_any_content {
        top_level_commas + 1
    } else {
        0
    }
}
