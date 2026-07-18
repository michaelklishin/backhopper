// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanners for the two Erlang attributes that drive
//! `HeaderFileMissing` and `BehaviourModuleMissing`.
//!
//! Both attributes have stable, line-oriented shapes:
//!
//!   * `-behaviour(custom_ct_hook).` (also spelled `-behavior(...)` in
//!     OTP-style sources)
//!   * `-include("rel/path.hrl").` and
//!     `-include_lib("app/include/file.hrl").`
//!
//! We scan the source byte by byte, balancing strings, char literals,
//! and `%` line comments. Anything more complex (macro-substituted
//! attributes, `parse_transform`-injected values) is not visible to the
//! scanner: those cases are left to a follow-up
//! `--deep-helper-resolution` pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str;
use std::str::FromStr;

use backhopper_erlang_scan::{
    count_top_level_commas, parse_callable_signature, take_balanced_parens,
};

use crate::compat::target_tree_index::TargetTreeIndex;
use crate::compat::test_suite::module_resolves;
use crate::model::names::{Arity, FunctionName, ModuleName, RelativePath, TypeName};
use crate::model::verdict::IncludeDirective;
use crate::snapshot::spec_normalize::normalize_signature;

/// One scanned `-behaviour(M)` declaration, with the line number for
/// the user-facing report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviourRef {
    pub behaviour: ModuleName,
    pub line: u32,
}

/// One scanned `-include(...)` or `-include_lib(...)` declaration,
/// with the line number for the user-facing report.
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

/// One `-define(NAME, ...)` or `-define(NAME(Args), ...)` form:
/// `params` is `None` for the object-like form, and `body` is the
/// definition text with comments stripped and whitespace collapsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDef {
    pub params: Option<usize>,
    pub body: String,
}

/// Every `-define` in `src` with its normalized body, keyed by name.
/// A name defined more than once (`-ifdef` branches, or coexisting
/// object-like and function-like forms) keeps every definition, so the
/// caller can withhold instead of guessing which one is live.
pub fn extract_defined_macro_values(src: &str) -> BTreeMap<String, Vec<MacroDef>> {
    let mut out: BTreeMap<String, Vec<MacroDef>> = BTreeMap::new();
    for hit in iter_attribute_bodies(src, &["define"]) {
        let Some((name, params, value)) = parse_macro_define(hit.body) else {
            continue;
        };
        out.entry(name).or_default().push(MacroDef {
            params,
            body: normalize_macro_body(value),
        });
    }
    out
}

/// Splits a `-define` body into name, parameter count, and value text.
fn parse_macro_define(body: &str) -> Option<(String, Option<usize>, &str)> {
    let trimmed = body.trim_start();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '@'))
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_owned();
    let rest = trimmed[name_end..].trim_start();
    let (params, after) = if rest.starts_with('(') {
        let (args, after_args) = take_balanced_parens(rest)?;
        let count = if args.trim().is_empty() {
            0
        } else {
            count_top_level_commas(args) + 1
        };
        (Some(count), after_args.trim_start())
    } else {
        (None, rest)
    };
    let value = after.strip_prefix(',')?;
    Some((name, params, value))
}

/// Comments stripped, whitespace runs collapsed to one space, ends
/// trimmed: enough to equate reformatting without equating bodies that
/// differ.
fn normalize_macro_body(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    let mut in_string = false;
    let mut in_atom = false;
    while let Some(c) = chars.next() {
        if in_string || in_atom {
            out.push(c);
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '"' if in_string => in_string = false,
                '\'' if in_atom => in_atom = false,
                _ => {}
            }
            continue;
        }
        match c {
            '%' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            '"' => {
                in_string = true;
                out.push(c);
            }
            '\'' => {
                in_atom = true;
                out.push(c);
            }
            '$' => {
                out.push(c);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            c if c.is_whitespace() => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            c => out.push(c),
        }
    }
    out.trim().to_owned()
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
            b'"' => {
                let end = skip_string(bytes, i, b'"');
                line += newline_count(&bytes[i..end]);
                i = end;
            }
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => {
                let end = skip_string(bytes, i, b'\'');
                line += newline_count(&bytes[i..end]);
                i = end;
            }
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

/// Every scanned `name(Args)` occurrence, classified into definitions and
/// calls. Qualified (`mod:f(`), macro (`?f(`), and `fun name(` forms
/// are skipped, as are reserved words. Variables (`Var(...)`) and
/// attribute or spec forms (`-export(...)`, `-spec ... term()`) hold no
/// local calls, so they are consumed without emitting a signature.
pub fn extract_function_signatures(src: &str) -> Vec<FunctionSignature> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    // inside an attribute or spec form type names look like zero-arity calls
    let mut in_attribute = false;
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
            b'-' if !in_attribute && at_form_start(bytes, i) => {
                in_attribute = true;
                i += 1;
            }
            b'.' if in_attribute && is_form_terminator(bytes, i) => {
                in_attribute = false;
                i += 1;
            }
            b if b.is_ascii_uppercase() || b == b'_' => {
                // consume the whole variable name so its tail is not re-lexed
                while i < bytes.len() && is_name_char(bytes[i]) {
                    i += 1;
                }
            }
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
                        if !in_attribute {
                            let arity = top_level_arity(&bytes[j + 1..close]);
                            let is_definition = followed_by_clause_arrow(bytes, close + 1);
                            out.push(FunctionSignature {
                                name: String::from_utf8_lossy(name).into_owned(),
                                arity,
                                line,
                                is_definition,
                            });
                        }
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

/// True when only whitespace precedes `pos` back to the last newline:
/// a `-` here opens an attribute or spec form, not a subtraction.
fn at_form_start(bytes: &[u8], pos: usize) -> bool {
    bytes[..pos]
        .iter()
        .rev()
        .take_while(|&&b| b != b'\n')
        .all(|&b| b.is_ascii_whitespace())
}

/// True when `.` at `pos` ends a form: followed by whitespace or end of
/// input, not a `..` range or a `mod.field` dot.
fn is_form_terminator(bytes: &[u8], pos: usize) -> bool {
    match bytes.get(pos + 1) {
        None => true,
        Some(b) => b.is_ascii_whitespace(),
    }
}

/// A function brought into a module's namespace by `-import(Module,
/// [f/a, ...])`. The module is retained so a patch-added import can be
/// resolved as the cross-module reference `m:f/a` it actually is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportedFunction {
    pub module: ModuleName,
    pub function: FunctionName,
    pub arity: Arity,
}

/// The functions imported via `-import(mod, [f/a, ...])`, each carrying
/// its source module. An entry whose module or name fails newtype
/// validation is dropped: it would not compile, so nothing resolves
/// against it.
pub fn extract_imports(src: &str) -> BTreeSet<ImportedFunction> {
    let mut out = BTreeSet::new();
    for hit in iter_attribute_bodies(src, &["import"]) {
        let Some(open) = hit.body.find('[') else {
            continue;
        };
        let Some(close) = hit.body[open..].find(']') else {
            continue;
        };
        let module_text = hit.body[..open].trim().trim_end_matches(',').trim();
        let Ok(module) = ModuleName::from_str(module_text) else {
            continue;
        };
        for entry in hit.body[open + 1..open + close].split(',') {
            if let Some((name, arity)) = parse_fun_arity(entry)
                && let (Ok(function), Ok(arity)) =
                    (FunctionName::from_str(&name), Arity::try_from(arity))
            {
                out.insert(ImportedFunction {
                    module: module.clone(),
                    function,
                    arity,
                });
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

/// A module's exported `f/a` set, with `complete = false` when the true
/// export surface cannot be read from source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSet {
    pub exports: BTreeSet<(FunctionName, Arity)>,
    pub complete: bool,
}

/// Functions a module exports via `-export([f/a, ...])`. `complete` is
/// false when the export surface is not fully knowable from source:
/// `-compile(export_all)` (or `nowarn_export_all`), a declared
/// `parse_transform` (which can inject exports), or an `-export` entry
/// naming a macro the scanner cannot expand. A `complete = false` set
/// means the caller must withhold rather than treat an unlisted `f/a`
/// as undefined.
pub fn extract_exports(src: &str) -> ExportSet {
    let mut exports = BTreeSet::new();
    let mut complete = true;
    for hit in iter_attribute_bodies(src, &["compile"]) {
        if hit.body.contains("export_all") || hit.body.contains("parse_transform") {
            complete = false;
        }
    }
    for hit in iter_attribute_bodies(src, &["export"]) {
        let Some(open) = hit.body.find('[') else {
            continue;
        };
        let Some(close) = hit.body[open..].find(']') else {
            continue;
        };
        for entry in hit.body[open + 1..open + close].split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            // A macro in the export list hides which f/a it names.
            if entry.contains('?') {
                complete = false;
                continue;
            }
            if let Some((name, arity)) = parse_fun_arity(entry)
                && let (Ok(function), Ok(arity)) =
                    (FunctionName::from_str(&name), Arity::try_from(arity))
            {
                exports.insert((function, arity));
            }
        }
    }
    ExportSet { exports, complete }
}

/// A type a module exports, and the line its entry sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedType {
    pub name: TypeName,
    pub arity: Arity,
    pub line: u32,
}

/// A module's `-export_type([t/a, ...])` set. `complete` is false when
/// the surface cannot be read from source: a declared `parse_transform`,
/// or an entry naming a macro the scanner cannot expand.
/// `-compile(export_all)` exports functions, not types, so it does not
/// make this set incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedTypeSet {
    pub types: Vec<ExportedType>,
    pub complete: bool,
}

/// The types a module exports. An entry whose name fails newtype
/// validation is dropped: it would not compile, so nothing resolves
/// against it.
pub fn extract_exported_types(src: &str) -> ExportedTypeSet {
    let mut types = Vec::new();
    let mut complete = !declares_parse_transform(src);
    for hit in iter_attribute_bodies(src, &["export_type"]) {
        let Some(open) = hit.body.find('[') else {
            continue;
        };
        let Some(close) = hit.body[open..].find(']') else {
            continue;
        };
        for entry in hit.body[open + 1..open + close].split(',') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            // A macro in the export list hides which type it names.
            if entry.contains('?') {
                complete = false;
                continue;
            }
            if let Some((name, arity)) = parse_fun_arity(entry)
                && let (Ok(name), Ok(arity)) = (TypeName::from_str(&name), Arity::try_from(arity))
            {
                types.push(ExportedType {
                    name,
                    arity,
                    line: hit.line,
                });
            }
        }
    }
    ExportedTypeSet { types, complete }
}

/// All three declare a name; their visibility and equivalence rules
/// differ, which does not bear on whether an export resolves.
const TYPE_DECL_KEYWORDS: [&[u8]; 3] = [b"type", b"opaque", b"nominal"];

/// True when the source declares a macro-expanded attribute form
/// (`-?PROTOCOL_TYPE(t()).`), which can expand to any attribute:
/// `rabbit_framing` declares its whole type surface this way. No
/// attribute-derived set from such a file is knowable.
pub fn has_macro_expanded_attribute(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b'-' if at_form_start(bytes, i) => {
                let rest = &bytes[i + 1..];
                let after_space = rest.iter().position(|b| !b.is_ascii_whitespace());
                if after_space.is_some_and(|p| rest[p] == b'?') {
                    return true;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    false
}

/// The types a module declares. Walks form starts, not parenthesised
/// attribute bodies: `-type t() :: term().` is not an `-attr(...)`
/// form, though the equivalent `-type(t() :: term()).` is also legal.
pub fn extract_defined_types(src: &str) -> BTreeSet<(TypeName, Arity)> {
    let bytes = src.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b'-' if at_form_start(bytes, i) => {
                let Some(keyword) = TYPE_DECL_KEYWORDS
                    .iter()
                    .find(|k| at_keyword(bytes, i + 1, k))
                else {
                    i += 1;
                    continue;
                };
                let body_start = i + 1 + keyword.len();
                let body_end = form_end(bytes, body_start);
                let body = str::from_utf8(&bytes[body_start..body_end]).unwrap_or("");
                if let Some((name, arity)) = parse_type_name_arity(strip_outer_parens(body))
                    && let (Ok(name), Ok(arity)) =
                        (TypeName::from_str(&name), Arity::try_from(arity))
                {
                    out.insert((name, arity));
                }
                i = body_end;
            }
            _ => i += 1,
        }
    }
    out
}

/// `name(Arg, ...) :: Body` to `(name, arity)`. The `::` is required:
/// without it the form does not declare a type.
fn parse_type_name_arity(body: &str) -> Option<(String, usize)> {
    let trimmed = body.trim();
    let name_end = trimmed
        .bytes()
        .position(|b| !is_name_char(b))
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = &trimmed[..name_end];
    let (args, rest) = take_balanced_parens(trimmed[name_end..].trim_start())?;
    rest.trim_start().strip_prefix("::")?;
    let arity = if args.trim().is_empty() {
        0
    } else {
        count_top_level_commas(args) + 1
    };
    Some((name.to_owned(), arity))
}

/// A module's `-spec` signatures, keyed by `(function, arity)`.
pub type SpecTable = BTreeMap<(FunctionName, Arity), String>;

/// A module's `-spec name(Args) -> Ret.` signatures, keyed by
/// `(function, arity)`. Parsing is `backhopper-erlang-scan`'s
/// `parse_callable_signature`: the same grammar the snapshot pipeline
/// uses, so the live axis and the snapshot axis cannot disagree on
/// what a spec is or which arity it has. Module-qualified forms parse
/// to `None` and are simply absent, which withholds downstream.
pub fn extract_specs(src: &str) -> SpecTable {
    extract_signature_forms(src, b"spec")
}

/// A behaviour module's `-callback name(Args) -> Ret.` signatures,
/// keyed by `(function, arity)`. Same walker and grammar as
/// `extract_specs`.
pub fn extract_callbacks(src: &str) -> SpecTable {
    extract_signature_forms(src, b"callback")
}

/// The `(function, arity)` pairs a behaviour lists in
/// `-optional_callbacks([f/a, ...])`.
pub fn extract_optional_callbacks(src: &str) -> BTreeSet<(FunctionName, Arity)> {
    let mut out = BTreeSet::new();
    for hit in iter_attribute_bodies(src, &["optional_callbacks"]) {
        let Some(open) = hit.body.find('[') else {
            continue;
        };
        let Some(close) = hit.body[open..].find(']') else {
            continue;
        };
        for entry in hit.body[open + 1..open + close].split(',') {
            if let Some((name, arity)) = parse_fun_arity(entry)
                && let (Ok(function), Ok(arity)) =
                    (FunctionName::from_str(&name), Arity::try_from(arity))
            {
                out.insert((function, arity));
            }
        }
    }
    out
}

fn extract_signature_forms(src: &str, keyword: &[u8]) -> SpecTable {
    let bytes = src.as_bytes();
    let mut out = SpecTable::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b'-' if at_form_start(bytes, i) && at_keyword(bytes, i + 1, keyword) => {
                let body_start = i + 1 + keyword.len();
                let body_end = form_end(bytes, body_start);
                let body = str::from_utf8(&bytes[body_start..body_end]).unwrap_or("");
                if let Some(sig) = parse_callable_signature(strip_outer_parens(body))
                    && let Ok(function) = FunctionName::from_str(&sig.name)
                {
                    out.insert(
                        (function, Arity::new(sig.arity)),
                        normalize_signature(&sig.signature),
                    );
                }
                i = body_end;
            }
            _ => i += 1,
        }
    }
    out
}

/// True when `keyword` starts at `pos` and is not a prefix of a longer
/// attribute name (`-specs_of(...)` must not match `spec`).
fn at_keyword(bytes: &[u8], pos: usize, keyword: &[u8]) -> bool {
    bytes[pos..].starts_with(keyword)
        && !bytes
            .get(pos + keyword.len())
            .copied()
            .is_some_and(is_attribute_name_char)
}

/// The parenthesised attribute form: `-spec(f(Args) -> Ret).` wraps
/// the body once; the snapshot pipeline strips the same layer.
fn strip_outer_parens(body: &str) -> &str {
    let trimmed = body.trim();
    match trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        Some(inner) => inner.trim(),
        None => trimmed,
    }
}

/// Byte index of the `.` ending the form that starts at `start`, or
/// end of input; comments, strings, quoted atoms, and char literals
/// are skipped.
fn form_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => i = skip_string(bytes, i, b'"'),
            b'$' => i += 2.min(bytes.len() - i),
            b'\'' => i = skip_string(bytes, i, b'\''),
            b'.' if is_form_terminator(bytes, i) => return i,
            _ => i += 1,
        }
    }
    i
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
/// clause arrow (`->`) or a `when` guard, which marks a definition.
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
            // binary delimiters << >> nest like brackets, so their commas do not separate arguments
            b'<' if args.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 2;
                continue;
            }
            b'>' if args.get(i + 1) == Some(&b'>') => {
                depth -= 1;
                i += 2;
                continue;
            }
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
/// `test_helper_search_paths` globs as the SUITE helper resolver.
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
            let end = skip_string(bytes, i, b'"');
            line += newline_count(&bytes[i..end]);
            i = end;
            continue;
        }
        if b == b'\'' {
            let end = skip_string(bytes, i, b'\'');
            line += newline_count(&bytes[i..end]);
            i = end;
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
                line += newline_count(&bytes[i..=body_end]);
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

fn newline_count(span: &[u8]) -> u32 {
    let mut n = 0;
    for &b in span {
        if b == b'\n' {
            n += 1;
        }
    }
    n
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
