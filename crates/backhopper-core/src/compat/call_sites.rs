// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Call-site extractor for Erlang source lines.
//!
//! Given a hunk line of source text, finds references to:
//!   * `mod:fun(args)` calls
//!   * `?MACRO_USE`
//!   * `#record_use{}`
//! and definitions:
//!   * `name(...)` heads on lines that start at column 0 (function clauses)
//!
//! Also classifies dynamic dispatch that the static analyzer cannot resolve:
//!   * apply-family BIFs (`apply`, `spawn`, `spawn_link`, ...)
//!   * variable-module and variable-function calls (`Mod:foo(...)`, `mod:F(...)`)
//!
//! These flow into a separate diagnostic envelope and never feed `Verdict`.

use std::str::FromStr;
use std::sync::OnceLock;

use regex::Regex;

use crate::compat::arg_shape::ArgShape;
use crate::erlang_macros::{MacroTable, expand_value_macro_to_atom, expand_value_macro_to_mf};
use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName};
use crate::model::symbol::SymbolRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicCall {
    /// One of the apply-family BIFs: `apply/2`, `apply/3`, `spawn/3`,
    /// `spawn_link/3`, `spawn_monitor/3`, `spawn_opt/4`, `hibernate/3`.
    Apply,
    /// A call whose module or function name is a variable: `Mod:foo(...)`,
    /// `mod:F(...)`, or `Mod:F(...)`.
    VariableDispatch,
}

fn call_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b([a-z][a-zA-Z0-9_@]*)\s*:\s*([a-z][a-zA-Z0-9_@]*)\s*\(").expect("regex")
    })
}

fn record_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"#([a-z][a-zA-Z0-9_@]*)\b").expect("regex"))
}

fn macro_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\?([A-Z_][A-Za-z0-9_]*)\b").expect("regex"))
}

fn fun_def_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([a-z][a-zA-Z0-9_@]*)\s*\(").expect("regex"))
}

fn apply_bif_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b(apply|spawn|spawn_link|spawn_monitor|spawn_opt|hibernate)\s*\(")
            .expect("regex")
    })
}

fn var_module_call_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b[A-Z][a-zA-Z0-9_@]*\s*:\s*[a-zA-Z_][a-zA-Z0-9_@]*\s*\(").expect("regex")
    })
}

fn var_function_call_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b[a-z][a-zA-Z0-9_@]*\s*:\s*[A-Z][a-zA-Z0-9_@]*\s*\(").expect("regex")
    })
}

pub fn extract_into(line: &str, out: &mut Vec<SymbolRef>) {
    extract_into_with_macros(line, empty_macros(), out);
}

fn empty_macros() -> &'static MacroTable {
    static EMPTY: OnceLock<MacroTable> = OnceLock::new();
    EMPTY.get_or_init(MacroTable::new)
}

/// Same as `extract_into` but consults `macros` so `?Name`-style
/// references resolve into concrete `mod:fn(...)` symbols when the
/// macro's body is a bare atom (module) or `mod:fn` pair. Apply-family
/// BIFs whose `M`, `F`, and arg-list slots are literals also resolve
/// into the matching `Mfa` and land in `out` as normal function refs.
pub fn extract_into_with_macros(line: &str, macros: &MacroTable, out: &mut Vec<SymbolRef>) {
    for caps in call_re().captures_iter(line) {
        let module = &caps[1];
        let function = &caps[2];
        let after = &line[caps.get(0).expect("capture").end()..];
        let arity = approximate_arity(after);
        if let (Ok(m), Ok(f)) = (
            ModuleName::from_str(module),
            FunctionName::from_str(function),
        ) {
            out.push(SymbolRef::function(Mfa::new(m, f, Arity::new(arity))));
        }
    }
    for caps in record_re().captures_iter(line) {
        if let Ok(name) = RecordName::from_str(&caps[1]) {
            out.push(SymbolRef::record(name));
        }
    }
    for caps in macro_re().captures_iter(line) {
        let name = &caps[1];
        out.push(SymbolRef::macro_use(name.to_owned()));
        let head_end = caps.get(0).expect("capture").end();
        let tail = &line[head_end..];
        try_resolve_macro_call(name, tail, macros, out);
    }
    extract_apply_family_into(line, macros, out);
}

fn try_resolve_macro_call(name: &str, tail: &str, macros: &MacroTable, out: &mut Vec<SymbolRef>) {
    let trimmed = tail.trim_start();
    if let Some(rest) = trimmed.strip_prefix('(') {
        let (args, _) = extract_arg_shapes(rest);
        if let Some((m, f)) = expand_value_macro_to_mf(macros, name) {
            if let (Ok(mn), Ok(fn_)) = (ModuleName::from_str(&m), FunctionName::from_str(&f)) {
                let arity = u8::try_from(args.len()).unwrap_or(u8::MAX);
                out.push(SymbolRef::function(Mfa::new(mn, fn_, Arity::new(arity))));
            }
        }
        return;
    }
    if let Some(rest) = trimmed.strip_prefix(':') {
        let rest = rest.trim_start();
        let (function_str, after_fn) = split_lowercase_ident(rest);
        if function_str.is_empty() {
            return;
        }
        let after = after_fn.trim_start();
        if let Some(after_open) = after.strip_prefix('(') {
            if let Some(m) = expand_value_macro_to_atom(macros, name) {
                if let (Ok(mn), Ok(fn_)) = (
                    ModuleName::from_str(&m),
                    FunctionName::from_str(function_str),
                ) {
                    let arity = approximate_arity(after_open);
                    out.push(SymbolRef::function(Mfa::new(mn, fn_, Arity::new(arity))));
                }
            }
        }
    }
}

fn split_lowercase_ident(s: &str) -> (&str, &str) {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
        .unwrap_or(s.len());
    if end == 0 || !s.starts_with(|c: char| c.is_ascii_lowercase()) {
        return ("", s);
    }
    s.split_at(end)
}

fn extract_apply_family_into(line: &str, macros: &MacroTable, out: &mut Vec<SymbolRef>) {
    for caps in apply_family_re().captures_iter(line) {
        let name = &caps[1];
        let head_end = caps.get(0).expect("capture").end();
        let after_open = &line[head_end..];
        let raw_args = split_top_level_args(after_open);
        if let Some(mfa) = resolve_apply_family(name, &raw_args, macros) {
            out.push(SymbolRef::function(mfa));
        }
    }
}

fn apply_family_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"\b(?:erlang\s*:\s*)?(apply|spawn|spawn_link|spawn_monitor|spawn_opt|hibernate)\s*\(",
        )
        .expect("regex")
    })
}

fn resolve_apply_family(name: &str, args: &[&str], macros: &MacroTable) -> Option<Mfa> {
    let (m_raw, f_raw, a_raw) = match (name, args.len()) {
        ("apply", 3)
        | ("spawn" | "spawn_link" | "spawn_monitor" | "hibernate", 3)
        | ("spawn_opt", 4) => (args[0], args[1], args[2]),
        ("spawn" | "spawn_link" | "spawn_monitor", 4) | ("spawn_opt", 5) => {
            (args[1], args[2], args[3])
        }
        _ => return None,
    };
    let module = atom_or_macro_to_module(m_raw, macros)?;
    let function = atom_or_macro_to_function(f_raw, macros)?;
    let arity = literal_list_length(a_raw)?;
    Some(Mfa::new(module, function, Arity::new(arity)))
}

fn atom_or_macro_to_module(raw: &str, macros: &MacroTable) -> Option<ModuleName> {
    let s = raw.trim();
    if let Some(name) = s.strip_prefix('?') {
        return expand_value_macro_to_atom(macros, name)
            .and_then(|a| ModuleName::from_str(&a).ok());
    }
    if looks_like_lowercase_atom(s) {
        return ModuleName::from_str(s).ok();
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return ModuleName::from_str(&s[1..s.len() - 1]).ok();
    }
    None
}

fn atom_or_macro_to_function(raw: &str, macros: &MacroTable) -> Option<FunctionName> {
    let s = raw.trim();
    if let Some(name) = s.strip_prefix('?') {
        return expand_value_macro_to_atom(macros, name)
            .and_then(|a| FunctionName::from_str(&a).ok());
    }
    if looks_like_lowercase_atom(s) {
        return FunctionName::from_str(s).ok();
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return FunctionName::from_str(&s[1..s.len() - 1]).ok();
    }
    None
}

fn literal_list_length(raw: &str) -> Option<u8> {
    let s = raw.trim();
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    u8::try_from(count_top_level_items(s, '[', ']')).ok()
}

fn looks_like_lowercase_atom(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

/// Splits the source between an already-consumed opening `(` and its
/// matching `)` into top-level argument source slices. Tracks nested
/// parens, brackets, braces, strings, and quoted atoms.
fn split_top_level_args(after_open_paren: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let mut depth = 1i32;
    let mut in_str = false;
    let mut in_atom = false;
    let mut prev_backslash = false;
    let mut start = 0usize;
    let bytes = after_open_paren.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if prev_backslash {
            prev_backslash = false;
            i += 1;
            continue;
        }
        match c {
            b'\\' if in_str || in_atom => prev_backslash = true,
            b'"' if !in_atom => in_str = !in_str,
            b'\'' if !in_str => in_atom = !in_atom,
            b'(' | b'[' | b'{' if !in_str && !in_atom => depth += 1,
            b')' if !in_str && !in_atom => {
                depth -= 1;
                if depth == 0 {
                    let slice = &after_open_paren[start..i];
                    if !slice.trim().is_empty() || !out.is_empty() {
                        out.push(slice);
                    }
                    return out;
                }
            }
            b']' | b'}' if !in_str && !in_atom => depth -= 1,
            b',' if !in_str && !in_atom && depth == 1 => {
                out.push(&after_open_paren[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        let slice = &after_open_paren[start..];
        if !slice.trim().is_empty() {
            out.push(slice);
        }
    }
    out
}

/// Collects per-call argument shapes from one source line. Mirrors
/// `extract_into` for the `mod:fun(args)` case; classifies each actual
/// into an `ArgShape` so the analyzer can match against pin-side
/// clause-head patterns.
pub fn extract_call_args_into(line: &str, out: &mut Vec<(Mfa, Vec<ArgShape>)>) {
    for caps in call_re().captures_iter(line) {
        let module = &caps[1];
        let function = &caps[2];
        let head_end = caps.get(0).expect("capture").end();
        let after = &line[head_end..];
        let (args, _consumed) = extract_arg_shapes(after);
        let arity = u8::try_from(args.len()).unwrap_or(u8::MAX);
        if let (Ok(m), Ok(f)) = (
            ModuleName::from_str(module),
            FunctionName::from_str(function),
        ) {
            out.push((Mfa::new(m, f, Arity::new(arity)), args));
        }
    }
}

fn extract_arg_shapes(after_open_paren: &str) -> (Vec<ArgShape>, usize) {
    let mut args: Vec<ArgShape> = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut depth = 1i32;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_backslash = false;
    let mut i = 0usize;
    let bytes = after_open_paren.as_bytes();
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if prev_backslash {
            prev_backslash = false;
            i += 1;
            continue;
        }
        let in_quote = in_str || in_atom_quote;
        let starts_token = !in_quote
            && depth >= 1
            && current_start.is_none()
            && !ch.is_whitespace()
            && !matches!(ch, ',' | ')' | ']' | '}');
        if starts_token {
            current_start = Some(i);
        }
        match ch {
            '\\' if in_quote => {
                prev_backslash = true;
            }
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' | '[' | '{' if !in_quote => {
                depth += 1;
            }
            ')' if !in_quote => {
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = current_start {
                        args.push(classify_arg(&after_open_paren[start..i]));
                    }
                    return (args, i + 1);
                }
            }
            ']' | '}' if !in_quote => depth -= 1,
            ',' if !in_quote && depth == 1 => {
                if let Some(start) = current_start {
                    args.push(classify_arg(&after_open_paren[start..i]));
                }
                current_start = None;
            }
            _ => {}
        }
        i += 1;
    }
    (args, bytes.len())
}

fn classify_arg(raw: &str) -> ArgShape {
    let s = raw.trim();
    if s.is_empty() {
        return ArgShape::Unknown;
    }
    let first = s.chars().next().expect("non-empty");
    if first == '#' {
        let rest = &s[1..];
        let name_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if let Ok(record_name) = RecordName::from_str(name) {
            return ArgShape::Record { name: record_name };
        }
        return ArgShape::Unknown;
    }
    if first == '{' {
        let size = count_top_level_items(s, '{', '}');
        return ArgShape::Tuple { size };
    }
    if first == '[' {
        return ArgShape::List;
    }
    if first == '"' {
        return ArgShape::String;
    }
    if first == '<' && s.starts_with("<<") {
        return ArgShape::Binary;
    }
    if first == '\'' {
        let inner = s.trim_start_matches('\'').trim_end_matches('\'');
        return ArgShape::Atom {
            name: inner.to_string(),
        };
    }
    if first.is_ascii_digit()
        || (first == '-' && s.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
    {
        return if s.contains('.') {
            ArgShape::Float
        } else {
            ArgShape::Integer
        };
    }
    if first.is_ascii_uppercase() || first == '_' {
        return ArgShape::Variable;
    }
    if first.is_ascii_lowercase() {
        let bare = is_bare_identifier(s);
        if bare {
            return ArgShape::Atom {
                name: s.to_string(),
            };
        }
        if s.starts_with("fun ") || s == "fun" {
            return ArgShape::Fun;
        }
        return ArgShape::Unknown;
    }
    ArgShape::Unknown
}

fn count_top_level_items(s: &str, open: char, close: char) -> usize {
    let body = match (s.find(open), s.rfind(close)) {
        (Some(a), Some(b)) if b > a => &s[a + 1..b],
        _ => return 0,
    };
    if body.trim().is_empty() {
        return 0;
    }
    let mut depth = 0i32;
    let mut count = 1usize;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_backslash = false;
    for ch in body.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_str || in_atom_quote => prev_backslash = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' | '[' | '{' if !in_str && !in_atom_quote => depth += 1,
            ')' | ']' | '}' if !in_str && !in_atom_quote => depth -= 1,
            ',' if !in_str && !in_atom_quote && depth == 0 => count += 1,
            _ => {}
        }
    }
    count
}

fn is_bare_identifier(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

pub fn extract_dynamic_into(line: &str, out: &mut Vec<DynamicCall>) {
    for _ in apply_bif_re().captures_iter(line) {
        out.push(DynamicCall::Apply);
    }
    for _ in var_module_call_re().find_iter(line) {
        out.push(DynamicCall::VariableDispatch);
    }
    for _ in var_function_call_re().find_iter(line) {
        out.push(DynamicCall::VariableDispatch);
    }
}

pub fn extract_definitions_into(line: &str, out: &mut Vec<SymbolRef>) {
    if !line.starts_with(|c: char| c.is_ascii_lowercase()) {
        return;
    }
    let Some(caps) = fun_def_re().captures(line) else {
        return;
    };
    let head_end = caps.get(0).expect("capture").end();
    let after = &line[head_end..];
    let arity = approximate_arity(after);
    if let Ok(f) = FunctionName::from_str(&caps[1]) {
        out.push(SymbolRef::function(Mfa::new(
            local_placeholder_module().clone(),
            f,
            Arity::new(arity),
        )));
    }
}

fn local_placeholder_module() -> &'static ModuleName {
    static M: OnceLock<ModuleName> = OnceLock::new();
    M.get_or_init(|| ModuleName::from_str("_local").expect("valid"))
}

fn approximate_arity(after_open_paren: &str) -> u8 {
    let mut depth = 1i32;
    let mut count = 0u8;
    let mut saw_arg = false;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_backslash = false;
    for ch in after_open_paren.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_str || in_atom_quote => prev_backslash = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => depth += 1,
            ')' if !in_str && !in_atom_quote => {
                depth -= 1;
                if depth == 0 {
                    if saw_arg {
                        count = count.saturating_add(1);
                    }
                    return count;
                }
            }
            ',' if !in_str && !in_atom_quote && depth == 1 => {
                count = count.saturating_add(1);
                saw_arg = false;
            }
            c if !c.is_whitespace() && depth >= 1 => saw_arg = true,
            _ => {}
        }
    }
    count
}