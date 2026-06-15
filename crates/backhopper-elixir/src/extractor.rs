// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Top-level Elixir source extractor: text → typed `Module` records.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::OnceLock;

use backhopper_core::model::names::{Arity, FunctionName, ModuleName, TypeName};
use backhopper_core::model::snapshot::{
    CallbackSig, FunArity, HrlFile, Module, SpecSig, TypeDecl, Visibility,
};
use backhopper_core::snapshot::spec_normalize::normalize_signature;
use regex::Regex;
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSource {
    pub modules: Vec<Module>,
    pub headers: Vec<HrlFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ElixirExtractor {
    pub public_modules: Vec<String>,
    pub internal_modules: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("invalid utf-8 in {path:?} at byte offset {offset}")]
    InvalidUtf8 { path: PathBuf, offset: usize },
}

const ELIXIR_FILE_EXTS: &[&str] = &[".ex", ".exs"];

impl ElixirExtractor {
    pub fn new(public_modules: Vec<String>, internal_modules: Vec<String>) -> Self {
        Self {
            public_modules,
            internal_modules,
        }
    }

    pub fn extract_source<I>(&self, files: I) -> Result<ExtractedSource, ExtractError>
    where
        I: IntoIterator<Item = (PathBuf, Vec<u8>)>,
    {
        let mut modules: Vec<Module> = Vec::new();
        for (path, bytes) in files {
            let path_string = path.to_string_lossy().to_string();
            if !ELIXIR_FILE_EXTS
                .iter()
                .any(|ext| path_string.ends_with(ext))
            {
                continue;
            }
            let text = str::from_utf8(&bytes).map_err(|e| ExtractError::InvalidUtf8 {
                path: path.clone(),
                offset: e.valid_up_to(),
            })?;
            for m in self.extract_modules(text) {
                modules.push(m);
            }
        }
        Ok(ExtractedSource {
            modules,
            headers: Vec::new(),
        })
    }

    pub fn extract_modules(&self, source: &str) -> Vec<Module> {
        let stripped = strip_comments(source);
        let lines: Vec<String> = stripped.lines().map(str::to_owned).collect();
        let mut cursor = 0usize;
        let mut out = Vec::new();
        while cursor < lines.len() {
            if defmodule_re().is_match(&lines[cursor]) {
                collect_module_recursive(&lines, &mut cursor, "", self, &mut out);
            } else {
                cursor += 1;
            }
        }
        out
    }
}

fn defmodule_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*defmodule\s+([A-Z][A-Za-z0-9_.]*)\s+do\b").expect("regex"))
}

fn def_head_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\s*(def|defp|defmacro|defmacrop)\s+([a-z_][a-zA-Z0-9_?!]*)\s*(\(|do|,|\s*$)")
            .expect("regex")
    })
}

fn attr_callable_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\s*@(callback|spec)\s+([a-z_][a-zA-Z0-9_?!]*)\s*").expect("regex")
    })
}

fn attr_type_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\s*@(type|typep|opaque)\s+([a-z_][a-zA-Z0-9_]*)\s*").expect("regex")
    })
}

fn moduledoc_false_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\s*@moduledoc\s+false\s*$").expect("regex"))
}

fn collect_module_recursive(
    lines: &[String],
    cursor: &mut usize,
    parent: &str,
    ex: &ElixirExtractor,
    out: &mut Vec<Module>,
) {
    let header_line = &lines[*cursor];
    let Some(caps) = defmodule_re().captures(header_line) else {
        return;
    };
    let local = caps[1].to_owned();
    *cursor += 1;
    let full = if parent.is_empty() {
        local.clone()
    } else {
        format!("{parent}.{local}")
    };
    let Ok(module_name) = ModuleName::from_str(&full) else {
        warn!("invalid Elixir module name: {}", full);
        return;
    };
    let mut module = Module::new(module_name);
    let mut depth: i32 = 1;
    let mut hidden = false;
    let mut exports: BTreeSet<(String, u8)> = BTreeSet::new();
    let mut callbacks: Vec<CallbackSig> = Vec::new();
    let mut specs: Vec<SpecSig> = Vec::new();
    let mut types: Vec<TypeDecl> = Vec::new();
    while *cursor < lines.len() {
        let line: &str = &lines[*cursor];
        if defmodule_re().is_match(line) {
            collect_module_recursive(lines, cursor, &full, ex, out);
            continue;
        }
        *cursor += 1;
        if moduledoc_false_re().is_match(line) {
            hidden = true;
            continue;
        }
        if let Some(caps) = def_head_re().captures(line) {
            let kind = caps[1].to_owned();
            let fname = caps[2].to_owned();
            let after_name = &line[caps.get(0).expect("match").end() - 1..];
            let arity = function_head_arity(after_name);
            if matches!(kind.as_str(), "def" | "defmacro") {
                exports.insert((fname, arity));
            }
            depth += depth_delta_for_def(line);
            depth += extra_block_openers(line);
            depth -= count_inline_ends(line);
            if depth <= 0 {
                break;
            }
            continue;
        }
        if let Some(caps) = attr_callable_re().captures(line) {
            let kind = caps[1].to_owned();
            let fname = caps[2].to_owned();
            let after = line[caps.get(0).expect("match").end()..].to_owned();
            if !after.trim_start().starts_with('(') {
                continue;
            }
            let body = collect_balanced(lines, cursor, &after);
            if let Some((arity, signature)) = parse_attr_signature(&fname, &body)
                && let Ok(name) = FunctionName::from_str(&fname)
            {
                if kind == "callback" {
                    callbacks.push(CallbackSig {
                        name,
                        arity: Arity::new(arity),
                        signature,
                    });
                } else {
                    specs.push(SpecSig {
                        name,
                        arity: Arity::new(arity),
                        signature,
                    });
                }
            }
            continue;
        }
        if let Some(caps) = attr_type_re().captures(line) {
            let _kind = caps[1].to_owned();
            let tname = caps[2].to_owned();
            let after = line[caps.get(0).expect("match").end()..].to_owned();
            let trimmed_after = after.trim_start();
            if !(trimmed_after.starts_with('(') || trimmed_after.starts_with("::")) {
                continue;
            }
            let body = collect_balanced(lines, cursor, &after);
            if let Some((arity, rhs)) = parse_attr_type_decl(&tname, &body)
                && let Ok(name) = TypeName::from_str(&tname)
            {
                types.push(TypeDecl {
                    name,
                    arity: Arity::new(arity),
                    rhs: normalize_signature(&rhs),
                });
            }
            continue;
        }
        depth += extra_block_openers(line);
        depth -= count_inline_ends(line);
        if depth <= 0 {
            break;
        }
    }
    for (n, a) in exports {
        if let Ok(name) = FunctionName::from_str(&n) {
            module.exports.push(FunArity {
                name,
                arity: Arity::new(a),
            });
        }
    }
    module.callbacks = callbacks;
    module.specs = specs;
    module.types = types;
    let name_str = module.name.as_str();
    module.visibility = if ex.internal_modules.iter().any(|n| n == name_str)
        || (hidden && !ex.public_modules.iter().any(|n| n == name_str))
    {
        Visibility::Hidden
    } else {
        Visibility::Public
    };
    out.push(module);
}

fn function_head_arity(after_name: &str) -> u8 {
    if !after_name.starts_with('(') {
        return 0;
    }
    let inside = match take_balanced_parens(after_name) {
        Some((s, _)) => s,
        None => return 0,
    };
    let trimmed = inside.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let commas = count_top_level_commas(trimmed);
    (commas + 1).min(255) as u8
}

/// `body` starts at the `(` of the function head.
fn parse_attr_signature(name: &str, body: &str) -> Option<(u8, String)> {
    let trimmed = body.trim_start();
    let (args, rest) = take_balanced_parens(trimmed)?;
    let arity = if args.trim().is_empty() {
        0
    } else {
        (count_top_level_commas(args) + 1).min(255) as u8
    };
    let after = rest.trim_start();
    let rhs = after.strip_prefix("::").unwrap_or(after).trim();
    let signature = format!("{name}({}) :: {rhs}", args.trim());
    Some((arity, signature))
}

/// `body` starts at either `(` (parametric type) or `::` (zero-arity type).
fn parse_attr_type_decl(_name: &str, body: &str) -> Option<(u8, String)> {
    let trimmed = body.trim_start();
    let (args, rest) = if trimmed.starts_with('(') {
        take_balanced_parens(trimmed)?
    } else {
        ("", trimmed)
    };
    let arity = if args.trim().is_empty() {
        0
    } else {
        (count_top_level_commas(args) + 1).min(255) as u8
    };
    let after = rest.trim_start();
    let rhs = after.strip_prefix("::").unwrap_or(after).trim();
    Some((arity, rhs.to_owned()))
}

fn collect_balanced(lines: &[String], cursor: &mut usize, first: &str) -> String {
    let mut acc = String::from(first);
    while *cursor < lines.len() && needs_continuation(&acc, &lines[*cursor]) {
        acc.push('\n');
        acc.push_str(&lines[*cursor]);
        *cursor += 1;
    }
    acc
}

fn needs_continuation(acc: &str, next: &str) -> bool {
    if !is_balanced(acc) {
        return true;
    }
    let next_trimmed = next.trim_start();
    if next_trimmed.is_empty() {
        return false;
    }
    if next.starts_with(' ') || next.starts_with('\t') {
        let tail = acc.trim_end();
        if tail.ends_with("::") || tail.ends_with('|') || tail.ends_with(',') || tail.ends_with('+')
        {
            return true;
        }
        if next_trimmed.starts_with('|') || next_trimmed.starts_with("when ") {
            return true;
        }
    }
    false
}

fn is_elixir_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// In Elixir ?x is the codepoint of x, but ? is also the suffix of predicate
// names like valid?: treat it as a char literal only when it does not follow
// an identifier byte. Returns the byte length to skip, or None otherwise.
fn elixir_char_literal_len(bytes: &[u8], at: usize) -> Option<usize> {
    if at > 0 && is_elixir_ident_byte(bytes[at - 1]) {
        return None;
    }
    let next = at + 1;
    if next >= bytes.len() {
        return None;
    }
    if bytes[next] == b'\\' {
        return Some(3.min(bytes.len() - at));
    }
    Some(2)
}

fn is_balanced(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut paren = 0i32;
    let mut brace = 0i32;
    let mut brack = 0i32;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut i = 0usize;
    while i < bytes.len() {
        if prev_back {
            prev_back = false;
            i += 1;
            continue;
        }
        if bytes[i] == b'?'
            && !in_str
            && !in_atom_quote
            && let Some(len) = elixir_char_literal_len(bytes, i)
        {
            i += len;
            continue;
        }
        match bytes[i] as char {
            '\\' if in_str || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => paren += 1,
            ')' if !in_str && !in_atom_quote => paren -= 1,
            '{' if !in_str && !in_atom_quote => brace += 1,
            '}' if !in_str && !in_atom_quote => brace -= 1,
            '[' if !in_str && !in_atom_quote => brack += 1,
            ']' if !in_str && !in_atom_quote => brack -= 1,
            _ => {}
        }
        i += 1;
    }
    paren <= 0 && brace <= 0 && brack <= 0 && !in_str && !in_atom_quote
}

fn take_balanced_parens(s: &str) -> Option<(&str, &str)> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if prev_back {
            prev_back = false;
            i += 1;
            continue;
        }
        if bytes[i] == b'?'
            && !in_str
            && !in_atom_quote
            && let Some(len) = elixir_char_literal_len(bytes, i)
        {
            i += len;
            continue;
        }
        match bytes[i] as char {
            '\\' if in_str || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => depth += 1,
            ')' if !in_str && !in_atom_quote => {
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

fn count_top_level_commas(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut count = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if prev_back {
            prev_back = false;
            i += 1;
            continue;
        }
        if bytes[i] == b'?'
            && !in_str
            && !in_atom_quote
            && let Some(len) = elixir_char_literal_len(bytes, i)
        {
            i += len;
            continue;
        }
        match bytes[i] as char {
            '\\' if in_str || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => depth_paren += 1,
            ')' if !in_str && !in_atom_quote => depth_paren -= 1,
            '{' if !in_str && !in_atom_quote => depth_brace += 1,
            '}' if !in_str && !in_atom_quote => depth_brace -= 1,
            '[' if !in_str && !in_atom_quote => depth_brack += 1,
            ']' if !in_str && !in_atom_quote => depth_brack -= 1,
            ',' if !in_str
                && !in_atom_quote
                && depth_paren == 0
                && depth_brace == 0
                && depth_brack == 0 =>
            {
                count += 1;
            }
            _ => {}
        }
        i += 1;
    }
    count
}

fn depth_delta_for_def(line: &str) -> i32 {
    let trimmed = line.trim_end();
    if trimmed.ends_with(" do") || trimmed.ends_with("\tdo") {
        1
    } else {
        0
    }
}

fn extra_block_openers(line: &str) -> i32 {
    let trimmed = line.trim();
    if trimmed.starts_with("def ")
        || trimmed.starts_with("defp ")
        || trimmed.starts_with("defmacro ")
        || trimmed.starts_with("defmacrop ")
    {
        return 0;
    }
    let mut delta = 0i32;
    for keyword in [
        "if ", "case ", "cond ", "with ", "for ", "fn ", "try ", "receive ",
    ] {
        if trimmed.starts_with(keyword) && trimmed.ends_with(" do") {
            delta += 1;
        }
    }
    if trimmed == "do" {
        delta += 1;
    }
    delta
}

fn count_inline_ends(line: &str) -> i32 {
    let trimmed = line.trim();
    if trimmed == "end" { 1 } else { 0 }
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_heredoc: Option<&str> = None;
    for line in source.lines() {
        if let Some(closer) = in_heredoc {
            if line.trim_start().starts_with(closer) {
                in_heredoc = None;
            }
            out.push('\n');
            continue;
        }
        if let Some(closer) = detect_heredoc_open(line) {
            in_heredoc = Some(closer);
            out.push('\n');
            continue;
        }
        let mut in_str = false;
        let mut in_atom_quote = false;
        let mut prev_back = false;
        let mut cut = line.len();
        let lb = line.as_bytes();
        let mut i = 0usize;
        while i < lb.len() {
            if prev_back {
                prev_back = false;
                i += 1;
                continue;
            }
            if lb[i] == b'?'
                && !in_str
                && !in_atom_quote
                && let Some(len) = elixir_char_literal_len(lb, i)
            {
                i += len;
                continue;
            }
            match lb[i] as char {
                '\\' if in_str || in_atom_quote => prev_back = true,
                '"' if !in_atom_quote => in_str = !in_str,
                '\'' if !in_str => in_atom_quote = !in_atom_quote,
                '#' if !in_str && !in_atom_quote => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

fn detect_heredoc_open(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    let candidates = [
        ("\"\"\"", "\"\"\""),
        ("'''", "'''"),
        ("~S\"\"\"", "\"\"\""),
        ("~s\"\"\"", "\"\"\""),
        ("~S'''", "'''"),
        ("~s'''", "'''"),
    ];
    for (open, close) in candidates {
        if let Some(rest) = trimmed.strip_prefix(open) {
            if rest.contains(close) {
                return None;
            }
            return Some(close);
        }
    }
    if let Some(idx) = line.find("\"\"\"")
        && !line[..idx].contains('"')
        && line[idx + 3..].find("\"\"\"").is_none()
    {
        return Some("\"\"\"");
    }
    None
}
