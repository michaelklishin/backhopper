//! Canonical pretty-printer for `-spec`/`-callback`/`-type` right-hand sides.
//!
//! The job: produce a single canonical string for any given semantic
//! signature, so reformatting in source produces no snapshot diff. The
//! algorithm is intentionally simple: it normalizes whitespace, balances
//! brackets, wraps at top-level `|` past a width target.

const WRAP_WIDTH: usize = 80;
const CONTINUATION_INDENT: &str = "       ";

pub fn normalize_signature(input: &str) -> String {
    let collapsed = collapse_whitespace(input);
    if collapsed.len() <= WRAP_WIDTH {
        return collapsed;
    }
    wrap_top_level_alternatives(&collapsed)
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
    let trimmed = out.trim_end_matches(' ');
    trimmed.to_owned()
}

fn wrap_top_level_alternatives(input: &str) -> String {
    let pipes = top_level_pipe_positions(input);
    if pipes.is_empty() {
        return input.to_owned();
    }
    let mut out = String::with_capacity(input.len() + pipes.len() * 8);
    let mut last = 0usize;
    for &pos in &pipes {
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
