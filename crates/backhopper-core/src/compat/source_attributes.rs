// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanners for the two Erlang attributes that drive
//! candidate 5 (`HeaderFileMissing`) and candidate 6
//! (`BehaviourModuleMissing`) of `018_ci_signal_gaps`.
//!
//! Both attributes have stable, line-oriented shapes:
//!
//!   * `-behaviour(custom_ct_hook).` (also spelled `-behavior(...)` in
//!     OTP-style sources)
//!   * `-include("rel/path.hrl").` and
//!     `-include_lib("app/include/file.hrl").`
//!
//! We scan for these by walking the source byte by byte, balancing
//! strings, char literals, and `%` line comments. Anything more
//! complex (macro-substituted attributes, `parse_transform`-injected
//! values) is invisible to the scanner: those cases land in a
//! follow-up `--deep-helper-resolution` pass.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str;

use crate::compat::target_tree_index::TargetTreeIndex;
use crate::compat::test_suite::module_resolves;
use crate::model::names::{ModuleName, RelativePath};
use crate::model::verdict::IncludeDirective;

/// One `-behaviour(M)` declaration the scanner saw, with the line
/// number for the user-facing report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviourRef {
    pub behaviour: ModuleName,
    pub line: u32,
}

/// One `-include(...)` or `-include_lib(...)` declaration the scanner
/// saw, with the line number for the user-facing report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeRef {
    pub directive: IncludeDirective,
    pub line: u32,
}

/// One scanned `?MACRO` or `#record` use, with its line for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedUse {
    pub name: String,
    pub line: u32,
}

/// Macro names defined via `-define(NAME, ...)` or
/// `-define(NAME(Args), ...)`. Conditional nesting is ignored: a macro
/// with any `-define` counts as present, so the resolver never
/// false-flags a defined macro (the rare inactive-branch define is the
/// one documented miss).
pub fn extract_defined_macros(src: &str) -> BTreeSet<String> {
    iter_attribute_bodies(src, &["define"])
        .into_iter()
        .filter_map(|hit| parse_macro_define_name(hit.body))
        .collect()
}

/// Every `?MACRO` reference in `src`, comments and strings skipped.
/// `??NAME` (stringify) counts as a use of `NAME`.
pub fn extract_macro_uses(src: &str) -> Vec<ScannedUse> {
    scan_uses(src, b'?', |bytes, j| {
        let mut k = j;
        while k < bytes.len() && bytes[k] == b'?' {
            k += 1;
        }
        let start = k;
        while k < bytes.len() && is_name_char(bytes[k]) {
            k += 1;
        }
        (start, k)
    })
}

/// Every `#record` use in `src` (creation, match, update, field
/// access), comments and strings skipped. `#{...}` maps have no name
/// and are ignored.
pub fn extract_record_uses(src: &str) -> Vec<ScannedUse> {
    scan_uses(src, b'#', |bytes, j| {
        let mut start = j;
        if bytes.get(start) == Some(&b'\'') {
            start += 1;
            let mut k = start;
            while k < bytes.len() && bytes[k] != b'\'' {
                k += 1;
            }
            return (start, k);
        }
        let mut k = start;
        while k < bytes.len() && is_name_char(bytes[k]) {
            k += 1;
        }
        (start, k)
    })
}

/// Record names defined via `-record(name, {...})`.
pub fn extract_defined_records(src: &str) -> BTreeSet<String> {
    iter_attribute_bodies(src, &["record"])
        .into_iter()
        .filter_map(|hit| parse_single_atom_argument(hit.body))
        .collect()
}

/// Walk `src` for `sigil`-prefixed uses, skipping comments, strings,
/// and char literals. `name_span` returns the byte range of the name
/// just past the sigil; an empty range (e.g. a `#{` map) yields no use.
fn scan_uses(
    src: &str,
    sigil: u8,
    name_span: impl Fn(&[u8], usize) -> (usize, usize),
) -> Vec<ScannedUse> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b if b == sigil => {
                let (start, end) = name_span(bytes, i + 1);
                if end > start {
                    out.push(ScannedUse {
                        name: String::from_utf8_lossy(&bytes[start..end]).into_owned(),
                        line,
                    });
                }
                i = end.max(i + 1);
            }
            _ => i += 1,
        }
    }
    out
}

/// One `name(Args)` occurrence: a clause head when followed by `->` or
/// `when`, a call otherwise. Arity is the top-level argument count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSignature {
    pub name: String,
    pub arity: usize,
    pub line: u32,
    pub is_definition: bool,
}

/// Every `name(Args)` the scanner saw, classified into definitions and
/// calls. Qualified (`mod:f(`), macro (`?f(`), and `fun name(` forms
/// are skipped, as are reserved words.
pub fn extract_function_signatures(src: &str) -> Vec<FunctionSignature> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b if b.is_ascii_lowercase() => {
                let start = i;
                let mut j = i;
                while j < bytes.len() && is_name_char(bytes[j]) {
                    j += 1;
                }
                let name = &bytes[start..j];
                let qualified = start > 0 && matches!(bytes[start - 1], b':' | b'?' | b'#');
                if bytes.get(j) == Some(&b'(') && !qualified && !is_reserved_word(name) {
                    if let Some(close) = match_parens(bytes, j + 1) {
                        let arity = top_level_arity(&bytes[j + 1..close]);
                        let is_definition = followed_by_clause_arrow(bytes, close + 1);
                        out.push(FunctionSignature {
                            name: String::from_utf8_lossy(name).into_owned(),
                            arity,
                            line,
                            is_definition,
                        });
                        line += count_newlines(&bytes[start..=close]);
                        i = close + 1;
                        continue;
                    }
                }
                i = j;
            }
            _ => i += 1,
        }
    }
    out
}

/// `(Module, Function, Arity)` triples imported via `-import(mod, [f/a,
/// ...])`, by function and arity (the module is irrelevant once
/// imported into the local namespace).
pub fn extract_imports(src: &str) -> BTreeSet<(String, usize)> {
    let mut out = BTreeSet::new();
    for hit in iter_attribute_bodies(src, &["import"]) {
        let Some(open) = hit.body.find('[') else {
            continue;
        };
        let Some(close) = hit.body[open..].find(']') else {
            continue;
        };
        for entry in hit.body[open + 1..open + close].split(',') {
            if let Some((name, arity)) = parse_fun_arity(entry) {
                out.insert((name, arity));
            }
        }
    }
    out
}

/// True when the module declares a `parse_transform`, which can inject
/// functions the scanner cannot see: the caller suppresses local-call
/// flagging for such a module.
#[must_use]
pub fn declares_parse_transform(src: &str) -> bool {
    iter_attribute_bodies(src, &["compile"])
        .iter()
        .any(|hit| hit.body.contains("parse_transform"))
}

fn parse_fun_arity(entry: &str) -> Option<(String, usize)> {
    let (name, arity) = entry.trim().split_once('/')?;
    let name = name.trim();
    if name.is_empty() || !name.bytes().all(is_name_char) {
        return None;
    }
    Some((name.to_owned(), arity.trim().parse().ok()?))
}

fn is_reserved_word(name: &[u8]) -> bool {
    matches!(
        name,
        b"after"
            | b"and"
            | b"andalso"
            | b"band"
            | b"begin"
            | b"bnot"
            | b"bor"
            | b"bsl"
            | b"bsr"
            | b"bxor"
            | b"case"
            | b"catch"
            | b"cond"
            | b"div"
            | b"end"
            | b"fun"
            | b"if"
            | b"let"
            | b"maybe"
            | b"not"
            | b"of"
            | b"or"
            | b"orelse"
            | b"receive"
            | b"rem"
            | b"try"
            | b"when"
            | b"xor"
    )
}

/// True when, after skipping whitespace, the bytes at `pos` begin a
/// clause arrow (`->`) or a `when` guard: the marks of a definition.
fn followed_by_clause_arrow(bytes: &[u8], pos: usize) -> bool {
    let mut i = pos;
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    bytes[i..].starts_with(b"->") || bytes[i..].starts_with(b"when")
}

fn top_level_arity(args: &[u8]) -> usize {
    if args.iter().all(|b| b.is_ascii_whitespace()) {
        return 0;
    }
    let mut depth = 0i32;
    let mut commas = 0usize;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'"' => {
                i = skip_string(args, i, b'"');
                continue;
            }
            b'\'' => {
                i = skip_string(args, i, b'\'');
                continue;
            }
            b'$' => {
                i += 2;
                continue;
            }
            b',' if depth == 0 => commas += 1,
            _ => {}
        }
        i += 1;
    }
    commas + 1
}

fn count_newlines(bytes: &[u8]) -> u32 {
    let mut n = 0u32;
    for &b in bytes {
        if b == b'\n' {
            n += 1;
        }
    }
    n
}

/// `erlc` predefines these, so a reference to one is never undefined.
#[must_use]
pub fn is_predefined_macro(name: &str) -> bool {
    matches!(
        name,
        "MODULE"
            | "MODULE_STRING"
            | "FILE"
            | "LINE"
            | "MACHINE"
            | "FUNCTION_NAME"
            | "FUNCTION_ARITY"
            | "OTP_RELEASE"
            | "FEATURE_AVAILABLE"
            | "FEATURE_ENABLED"
    )
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'@'
}

fn parse_macro_define_name(body: &str) -> Option<String> {
    let trimmed = body.trim_start();
    let mut chars = trimmed.char_indices();
    let (_, first) = chars.next()?;
    if first == '\'' {
        let end = trimmed[1..].find('\'')?;
        return Some(trimmed[1..=end].to_owned());
    }
    if !(first.is_ascii_alphanumeric() || first == '_' || first == '@') {
        return None;
    }
    let end = trimmed
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
        .unwrap_or(trimmed.len());
    Some(trimmed[..end].to_owned())
}

/// Extract every `-behaviour(M)` / `-behavior(M)` declaration in
/// `src`. Macro-valued and parse-transform-injected attributes are
/// invisible to the syntactic walk.
pub fn extract_behaviours(src: &str) -> Vec<BehaviourRef> {
    extract_attribute_atom_argument(src, &["behaviour", "behavior"])
        .into_iter()
        .filter_map(|(atom, line)| {
            ModuleName::new(atom)
                .ok()
                .map(|m| BehaviourRef { behaviour: m, line })
        })
        .collect()
}

/// Extract every `-include("…")` and `-include_lib("app/…")`
/// declaration in `src`. Only literal string arguments are
/// recognised; `-include(?MACRO)` is skipped.
pub fn extract_includes(src: &str) -> Vec<IncludeRef> {
    let mut out = Vec::new();
    for (name, path, line) in extract_attribute_string_argument(src, &["include", "include_lib"]) {
        let directive = match name.as_str() {
            "include" => IncludeDirective::Include { path },
            "include_lib" => IncludeDirective::IncludeLib { path },
            _ => continue,
        };
        out.push(IncludeRef { directive, line });
    }
    out
}

/// Resolve `behaviour` against the target tree using the same
/// `test_helper_search_paths` globs as the SUITE helper resolver
/// plus, in practice, the stdlib allowlist (callers filter ahead).
pub fn behaviour_resolves(
    target: &TargetTreeIndex,
    search_path_globs: &[String],
    behaviour: &ModuleName,
) -> bool {
    module_resolves(target, search_path_globs, behaviour)
}

/// Try to find the on-target path the include directive points at.
/// Returns `Ok(found)` on resolution, `Err(attempted_paths)` when none
/// of the candidate target paths exist.
///
/// `source_path` is the touched `.erl` or `.hrl` that carries the
/// directive: needed to resolve `-include(path)` relative to the
/// source file's directory.
pub fn resolve_include(
    target: &TargetTreeIndex,
    source_path: &RelativePath,
    directive: &IncludeDirective,
) -> Result<RelativePath, Vec<RelativePath>> {
    let candidates = include_candidates(source_path, directive);
    for c in &candidates {
        if target.contains_path(Path::new(c.as_str())) {
            return Ok(c.clone());
        }
    }
    Err(candidates)
}

/// Generate the ordered list of target paths an include might resolve
/// to. The first match wins in `resolve_include`.
fn include_candidates(
    source_path: &RelativePath,
    directive: &IncludeDirective,
) -> Vec<RelativePath> {
    match directive {
        IncludeDirective::Include { path } => {
            let source_dir = Path::new(source_path.as_str())
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let joined = source_dir.join(path);
            normalised_relative_path(&joined).into_iter().collect()
        }
        IncludeDirective::IncludeLib { path } => include_lib_candidates(path),
    }
}

/// `-include_lib("app/include/file.hrl")` resolves the leading
/// `<app>` component through OTP's lib search path. On the target
/// tree we map `<app>` to any of: `deps/<app>`, `apps/<app>`,
/// `lib/<app>` (OTP itself uses the last form). The remainder of the
/// path follows verbatim.
fn include_lib_candidates(path: &str) -> Vec<RelativePath> {
    let mut segs = path.splitn(2, '/');
    let app = match segs.next() {
        Some(s) if !s.is_empty() => s,
        _ => return Vec::new(),
    };
    let rest = match segs.next() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for prefix in ["deps", "apps", "lib"] {
        let candidate = PathBuf::from(prefix).join(app).join(rest);
        if let Some(rp) = normalised_relative_path(&candidate) {
            out.push(rp);
        }
    }
    out
}

fn normalised_relative_path(p: &Path) -> Option<RelativePath> {
    let s = p.to_string_lossy().replace('\\', "/");
    RelativePath::new(s).ok()
}

fn extract_attribute_atom_argument(src: &str, names: &[&str]) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for hit in iter_attribute_bodies(src, names) {
        if let Some(atom) = parse_single_atom_argument(hit.body) {
            out.push((atom, hit.line));
        }
    }
    out
}

fn extract_attribute_string_argument(src: &str, names: &[&str]) -> Vec<(String, String, u32)> {
    let mut out = Vec::new();
    for hit in iter_attribute_bodies(src, names) {
        if let Some(path) = parse_single_string_argument(hit.body) {
            out.push((hit.name.to_owned(), path, hit.line));
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct AttrHit<'a> {
    name: &'a str,
    body: &'a str,
    line: u32,
}

/// Iterate every `-<name>(...)` the source declares, where `<name>`
/// is one of `names`. `body` is the bytes between the opening `(` and
/// the matching `)`. Skips comments, strings, char literals.
fn iter_attribute_bodies<'a>(src: &'a str, names: &'a [&'a str]) -> Vec<AttrHit<'a>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if b == b'%' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'"' {
            i = skip_string(bytes, i, b'"');
            continue;
        }
        if b == b'\'' {
            i = skip_string(bytes, i, b'\'');
            continue;
        }
        if b == b'-'
            && let Some(name) = match_attribute_keyword(bytes, i + 1, names)
        {
            let name_end = i + 1 + name.len();
            let mut j = name_end;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b'(' {
                i = name_end;
                continue;
            }
            let body_start = j + 1;
            if let Some(body_end) = match_parens(bytes, body_start) {
                let body = str::from_utf8(&bytes[body_start..body_end]).unwrap_or("");
                out.push(AttrHit { name, body, line });
                i = body_end + 1;
                continue;
            }
            i = name_end;
            continue;
        }
        i += 1;
    }
    out
}

fn match_attribute_keyword<'a>(
    bytes: &[u8],
    start: usize,
    names: &'a [&'a str],
) -> Option<&'a str> {
    for name in names {
        let nb = name.as_bytes();
        if start + nb.len() > bytes.len() {
            continue;
        }
        if &bytes[start..start + nb.len()] != nb {
            continue;
        }
        let after = start + nb.len();
        let next = bytes.get(after).copied().unwrap_or(b' ');
        if !is_attribute_name_char(next) {
            return Some(name);
        }
    }
    None
}

fn is_attribute_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn match_parens(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1i32;
    let mut i = start;
    while i < bytes.len() && depth > 0 {
        let b = bytes[i];
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                i = skip_string(bytes, i, b'"');
                continue;
            }
            b'\'' => {
                i = skip_string(bytes, i, b'\'');
                continue;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn skip_string(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn parse_single_atom_argument(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if first == '\'' {
        let end = trimmed[1..].find('\'')?;
        return Some(trimmed[1..=end].to_owned());
    }
    if !first.is_ascii_lowercase() {
        return None;
    }
    let mut end = 1;
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '_' || c == '@' {
            end += c.len_utf8();
            continue;
        }
        break;
    }
    Some(trimmed[..end].to_owned())
}

fn parse_single_string_argument(body: &str) -> Option<String> {
    let trimmed = body.trim();
    let first = trimmed.chars().next()?;
    if first != '"' {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut i = 1usize;
    let mut out = String::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            out.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        if b == b'"' {
            return Some(out);
        }
        out.push(b as char);
        i += 1;
    }
    None
}
