//! `-define` table and macro-body expansion helpers.
//!
//! Macros are keyed by `(name, optional arity)`: value macros use
//! `arity = None`, parameterized macros use `arity = Some(n)`. Bodies
//! are kept as raw source text and re-scanned at use sites by callers
//! that want to find calls hidden behind macros.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MacroKey {
    pub name: String,
    pub arity: Option<u8>,
}

pub type MacroTable = BTreeMap<MacroKey, String>;

/// Parses the body of a `-define(...)` attribute. Accepts the raw
/// `(NAME, body)` form and the inner `NAME, body` form that
/// `handle_attribute`-style callers pass after stripping the parens.
pub fn parse_define(body: &str) -> Option<(MacroKey, String)> {
    let s = body.trim();
    let s = s
        .strip_prefix('(')
        .map(|x| x.strip_suffix(')').unwrap_or(x))
        .unwrap_or(s);
    let (name_part, value_part) = split_define_head(s)?;
    if let Some((name, params)) = parse_parameterized_head(name_part) {
        let arity = u8::try_from(params).ok()?;
        return Some((
            MacroKey {
                name,
                arity: Some(arity),
            },
            value_part.trim().to_owned(),
        ));
    }
    let name = name_part.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    Some((MacroKey { name, arity: None }, value_part.trim().to_owned()))
}

fn split_define_head(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut in_atom = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'\\' && (in_str || in_atom) {
            i += 2;
            continue;
        }
        match c {
            b'"' if !in_atom => in_str = !in_str,
            b'\'' if !in_str => in_atom = !in_atom,
            b'(' | b'[' | b'{' if !in_str && !in_atom => depth += 1,
            b')' | b']' | b'}' if !in_str && !in_atom => depth -= 1,
            b',' if depth == 0 && !in_str && !in_atom => {
                return Some((&s[..i], &s[i + 1..]));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_parameterized_head(s: &str) -> Option<(String, usize)> {
    let s = s.trim();
    let (name, after) = s.split_once('(')?;
    let after = after.strip_suffix(')')?;
    let name = name.trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let trimmed = after.trim();
    if trimmed.is_empty() {
        return Some((name, 0));
    }
    let params = trimmed.split(',').count();
    Some((name, params))
}

/// Resolves a value macro to a single lowercase atom when its body is
/// exactly that. Returns `None` for parameterized macros and bodies
/// that parse as something else.
pub fn expand_value_macro_to_atom(table: &MacroTable, name: &str) -> Option<String> {
    let body = table.get(&MacroKey {
        name: name.to_owned(),
        arity: None,
    })?;
    let trimmed = body.trim();
    if looks_like_lowercase_atom(trimmed) {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

/// Resolves a value macro to a `(module, function)` pair when its body
/// is `mod:fn`.
pub fn expand_value_macro_to_mf(table: &MacroTable, name: &str) -> Option<(String, String)> {
    let body = table.get(&MacroKey {
        name: name.to_owned(),
        arity: None,
    })?;
    let trimmed = body.trim();
    let (m, f) = trimmed.split_once(':')?;
    let m = m.trim();
    let f = f.trim();
    if looks_like_lowercase_atom(m) && looks_like_lowercase_atom(f) {
        Some((m.to_owned(), f.to_owned()))
    } else {
        None
    }
}

fn looks_like_lowercase_atom(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}
