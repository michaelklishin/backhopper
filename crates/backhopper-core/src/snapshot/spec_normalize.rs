// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonical pretty-printer for `-spec`/`-callback`/`-type` right-hand sides.
//!
//! The job: produce a single canonical string for any given semantic
//! signature, so reformatting in source produces no snapshot diff. The
//! algorithm is intentionally simple: it normalizes whitespace, balances
//! brackets, wraps at top-level `|` past a width target.

use backhopper_erlang_scan::{
    for_each_top_level_byte, quoted_atom_span, remove_line_comments, skip_char_literal_span,
    string_span,
};

const WRAP_WIDTH: usize = 80;
const CONTINUATION_INDENT: &str = "       ";

/// Erlang entry point: `$c` char literals, triple-quoted strings, and
/// sigils are lexed as such, so `$"` or `$,` cannot derail the walk.
pub fn normalize_signature(input: &str) -> String {
    let stripped = remove_line_comments(input);
    let collapsed = collapse_whitespace_erlang(&stripped);
    if collapsed.len() <= WRAP_WIDTH {
        return collapsed;
    }
    wrap_at(&collapsed, &erlang_pipe_positions(&collapsed))
}

/// Elixir entry point: `$` is an ordinary byte in Elixir type text, so
/// the char-literal-aware Erlang lexing must not apply.
pub fn normalize_elixir_signature(input: &str) -> String {
    let stripped = strip_line_comments(input);
    let collapsed = collapse_whitespace(&stripped);
    if collapsed.len() <= WRAP_WIDTH {
        return collapsed;
    }
    wrap_at(&collapsed, &top_level_pipe_positions(&collapsed))
}

fn erlang_pipe_positions(input: &str) -> Vec<usize> {
    let bytes = input.as_bytes();
    let mut positions = Vec::new();
    for_each_top_level_byte(input, |i| {
        if bytes[i] == b'|' {
            positions.push(i);
        }
        false
    });
    positions
}

/// Whitespace runs collapsed to one space, with string, quoted atom,
/// and char-literal spans copied verbatim so their contents stay exact.
fn collapse_whitespace_erlang(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let span = match bytes[i] {
            b'"' | b'~' => string_span(bytes, i),
            b'\'' => quoted_atom_span(bytes, i),
            b'$' => Some(skip_char_literal_span(bytes, i)),
            _ => None,
        };
        if let Some(span) = span {
            // a span may end inside a multi-byte char (`$ä` reads two bytes)
            let mut end = (i + span).min(bytes.len());
            while !input.is_char_boundary(end) {
                end += 1;
            }
            out.push_str(&input[i..end]);
            prev_space = false;
            i = end;
            continue;
        }
        let ch = input[i..].chars().next().expect("char at boundary");
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
        i += ch.len_utf8();
    }
    let new_len = out.trim_end_matches(' ').len();
    out.truncate(new_len);
    out
}

fn strip_line_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if prev_back {
            out.push(ch);
            prev_back = false;
            continue;
        }
        if ch == '\\' && (in_string || in_atom_quote) {
            out.push(ch);
            prev_back = true;
            continue;
        }
        if ch == '"' && !in_atom_quote {
            in_string = !in_string;
            out.push(ch);
            continue;
        }
        if ch == '\'' && !in_string {
            in_atom_quote = !in_atom_quote;
            out.push(ch);
            continue;
        }
        if ch == '%' && !in_string && !in_atom_quote {
            for c in chars.by_ref() {
                if c == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    for ch in input.chars() {
        if prev_back {
            out.push(ch);
            prev_back = false;
            continue;
        }
        if ch == '\\' && (in_string || in_atom_quote) {
            out.push(ch);
            prev_back = true;
            continue;
        }
        if ch == '"' && !in_atom_quote {
            in_string = !in_string;
            out.push(ch);
            prev_space = false;
            continue;
        }
        if ch == '\'' && !in_string {
            in_atom_quote = !in_atom_quote;
            out.push(ch);
            prev_space = false;
            continue;
        }
        if in_string || in_atom_quote {
            out.push(ch);
            prev_space = false;
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    let new_len = out.trim_end_matches(' ').len();
    out.truncate(new_len);
    out
}

fn wrap_at(input: &str, pipes: &[usize]) -> String {
    if pipes.is_empty() {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len() + pipes.len() * 8);
    let mut last = 0usize;
    for &pos in pipes {
        let segment = input[last..pos].trim_end();
        if last == 0 {
            out.push_str(segment);
        } else {
            out.push('\n');
            out.push_str(CONTINUATION_INDENT);
            out.push_str(segment.trim_start());
        }
        last = pos;
    }
    let tail = input[last..].trim_end();
    out.push('\n');
    out.push_str(CONTINUATION_INDENT);
    out.push_str(tail.trim_start());
    out
}

fn top_level_pipe_positions(input: &str) -> Vec<usize> {
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_was_backslash = false;
    let mut positions = Vec::new();
    for (i, ch) in input.char_indices() {
        if prev_was_backslash {
            prev_was_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_str || in_atom_quote => prev_was_backslash = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => depth_paren += 1,
            ')' if !in_str && !in_atom_quote => depth_paren -= 1,
            '{' if !in_str && !in_atom_quote => depth_brace += 1,
            '}' if !in_str && !in_atom_quote => depth_brace -= 1,
            '[' if !in_str && !in_atom_quote => depth_brack += 1,
            ']' if !in_str && !in_atom_quote => depth_brack -= 1,
            '|' if !in_str
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0 =>
            {
                positions.push(i);
            }
            _ => {}
        }
    }
    positions
}
