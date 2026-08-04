// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Call-site extractor for Erlang source lines.
//!
//! Given a hunk line of source text, finds references to:
//!   * `mod:fun(args)` calls
//!   * `fun mod:fun/N` remote fun references
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

use std::mem;
use std::str::FromStr;
use std::sync::OnceLock;

use regex::Regex;

use crate::compat::arg_shape::ArgShape;
use crate::erlang_macros::{MacroTable, expand_value_macro_to_atom, expand_value_macro_to_mf};
use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName, TypeName};
use crate::model::symbol::{RefContext, RefOrigin, SymbolRef};

// re-exported from the scan crate so the import path stays stable
pub use backhopper_erlang_scan::{
    ScanArity, ScannedArgs, scan_top_level_args, split_top_level_args,
};
use backhopper_erlang_scan::{
    count_top_level_items, hash_inside_number, is_bare_atom, opens_type_ref_context, scan_arity,
    skip_char_literal_span,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DynamicCall {
    /// One of the apply-family BIFs: `apply/2`, `apply/3`, `spawn/3`,
    /// `spawn_link/3`, `spawn_monitor/3`, `spawn_opt/4`, `hibernate/3`.
    Apply,
    /// A call whose module or function name is a variable: `Mod:foo(...)`,
    /// `mod:F(...)`, or `Mod:F(...)`.
    VariableDispatch,
}

pub(crate) fn call_re() -> &'static Regex {
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

// multi-line mode: a joined run holds several clause heads, each at line start
fn fun_def_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^([a-z][a-zA-Z0-9_@]*)\s*\(").expect("regex"))
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

/// One statically-named `mod:fun(Args)` call with its source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualifiedCallSite {
    pub mfa: Mfa,
    pub line: u32,
}

/// Every statically-named `mod:fun(Args)` call in `src`, with line and
/// arity. Type-attribute lines (`-spec`, `-callback`, `-type`,
/// `-opaque`) are skipped: a qualified `mod:t()` there is a type
/// reference, not a call. `line_map[i]` is text line `i`'s file line:
/// consecutive `Body` lines join into one scanned run only when
/// file-adjacent, so a call whose argument list wraps yields its exact
/// arity while fragments from different hunks never concatenate into a
/// false construct. The scanner's classification state persists across
/// gaps on purpose: resetting would misread a still-open `-spec` as
/// body text.
pub fn extract_qualified_calls(src: &str, line_map: &[u32]) -> Vec<QualifiedCallSite> {
    extract_qualified_calls_with_context(src, line_map, &line_context(src))
}

/// `extract_qualified_calls` over a precomputed per-line classification
/// rather than one derived from `src` alone: a caller that classified
/// against fuller context (a hunk's `Context` lines, which never enter
/// `src`) passes it here so an orphaned continuation line still
/// resolves against its real attribute region.
pub fn extract_qualified_calls_with_context(
    src: &str,
    line_map: &[u32],
    ctx: &[RefContext],
) -> Vec<QualifiedCallSite> {
    let mut out = Vec::new();
    for run in body_runs_from_context(src, line_map, ctx) {
        let mut calls = Vec::new();
        push_call_args_with_offsets(&run.text, &mut calls);
        for ((mfa, _shapes), offset) in calls {
            out.push(QualifiedCallSite {
                mfa,
                line: run_line_at(&run.line_starts, offset),
            });
        }
    }
    out
}

/// One joined run of file-adjacent body lines, comment-stripped, with
/// each line's byte offset into `text` paired with its 1-based text
/// line. Shared by the qualified-call and indirect-reference
/// extractors so both scan wrapped constructs whole.
#[derive(Debug)]
pub(crate) struct BodyRun {
    pub(crate) text: String,
    pub(crate) line_starts: Vec<(usize, u32)>,
}

/// The accumulate-and-flush core for body runs: `strip` removes each
/// line's comment and `is_body` classifies the stripped line;
/// consecutive body lines with adjacent file lines join into one run,
/// so fragments from different hunks never concatenate into a false
/// construct.
pub(crate) fn body_runs_with<'a>(
    src: &'a str,
    line_map: &[u32],
    strip: impl Fn(&'a str) -> &'a str,
    mut is_body: impl FnMut(&str) -> bool,
) -> Vec<BodyRun> {
    debug_assert_eq!(
        line_map.len(),
        src.lines().count(),
        "line map length must equal the line count"
    );
    let mut out = Vec::new();
    let mut run = String::new();
    let mut run_lines: Vec<(usize, u32)> = Vec::new();
    let mut prev_body_file_line: Option<u32> = None;
    for (idx, line) in src.lines().enumerate() {
        let stripped = strip(line);
        let body = is_body(stripped);
        let file_line = line_map.get(idx).copied();
        let adjacent =
            matches!((prev_body_file_line, file_line), (Some(prev), Some(cur)) if cur == prev + 1);
        if (!body || !adjacent) && !run.is_empty() {
            out.push(BodyRun {
                text: mem::take(&mut run),
                line_starts: mem::take(&mut run_lines),
            });
        }
        prev_body_file_line = body.then_some(file_line).flatten();
        if !body {
            continue;
        }
        run_lines.push((run.len(), (idx + 1) as u32));
        run.push_str(stripped);
        run.push('\n');
    }
    if !run.is_empty() {
        out.push(BodyRun {
            text: run,
            line_starts: run_lines,
        });
    }
    out
}

/// Per-line attribute-region classification of `src`'s own lines, with
/// no outside context: the classifier starts fresh and scans `src` top
/// to bottom. Correct when `src` already holds every line whose
/// classification matters; a caller whose relevant lines extend beyond
/// `src` (a hunk's `Context` lines, dropped from the added-only blob)
/// should classify those too and feed the result to
/// `body_runs_from_context` instead.
pub fn line_context(src: &str) -> Vec<RefContext> {
    let mut scanner = AttrCtxScanner::new();
    src.lines()
        .map(|line| scanner.classify(strip_line_comment(line)))
        .collect()
}

/// Joins consecutive `Body` lines into runs, breaking on attribute
/// context and on file-line gaps, from a precomputed classification
/// rather than one derived from `src` alone: `ctx[i]` is text line
/// `i`'s context.
pub(crate) fn body_runs_from_context(
    src: &str,
    line_map: &[u32],
    ctx: &[RefContext],
) -> Vec<BodyRun> {
    let mut idx = 0usize;
    body_runs_with(src, line_map, strip_line_comment, |_line| {
        // other-attribute lines scan like body: a `-define` body holds real calls
        let body = ctx.get(idx).is_some_and(|c| c.holds_calls());
        idx += 1;
        body
    })
}

/// The value of the last `(start, value)` entry whose start is at or
/// before `offset`, or `default` when `offset` precedes every entry.
/// Entries are sorted by start.
fn value_at_or_before<T: Copy>(entries: &[(usize, T)], offset: usize, default: T) -> T {
    match entries.partition_point(|(start, _)| *start <= offset) {
        0 => default,
        count => entries[count - 1].1,
    }
}

/// The `SymbolRef` for a scanned `m:f` call: exact arity when the
/// argument list terminated on the line, else any-arity.
fn scanned_call(scan: ScanArity, module: ModuleName, function: FunctionName) -> SymbolRef {
    match scan {
        ScanArity::Exact(arity) => {
            SymbolRef::function(Mfa::new(module, function, Arity::new(arity)))
        }
        ScanArity::Unterminated => SymbolRef::function_any_arity(module, function),
    }
}

// before the first entry, fall back to the first run's line, or 1 with no runs
pub(crate) fn run_line_at(run_lines: &[(usize, u32)], offset: usize) -> u32 {
    value_at_or_before(run_lines, offset, run_lines.first().map_or(1, |(_, l)| *l))
}

/// Cross-line classifier that decides which `RefContext` a hunk line
/// sits in. `-spec`, `-callback`, `-type`, and `-opaque` open a
/// type-attribute region; any other `-` attribute form opens an
/// other-attribute region. Both run until the terminating `.` at the
/// top level, and a single line may both open and close one. Multi-line
/// attributes flow through the stored region so their later lines
/// classify correctly.
///
/// A triple-quoted string suspends the close rule while it is open,
/// because documentation prose ends lines with full stops constantly
/// and would otherwise close the region at its first sentence.
#[derive(Debug, Default, Clone, Copy)]
pub struct AttrCtxScanner {
    region: Option<AttrRegion>,
}

/// The open region, if any. A multi-line string only exists inside an
/// attribute form, so it carries that form's context rather than
/// sitting in a second field that could contradict it.
#[derive(Debug, Clone, Copy)]
enum AttrRegion {
    /// An attribute form, closed by a line ending in `.`.
    Open(RefContext),
    /// A multi-line string in one, closed by a line whose first content
    /// is a run of at least `quotes` quotes.
    String { attr: RefContext, quotes: usize },
}

impl AttrCtxScanner {
    pub fn new() -> Self {
        Self { region: None }
    }

    pub fn classify(&mut self, line: &str) -> RefContext {
        match self.region {
            Some(AttrRegion::String { attr, quotes }) => {
                if line_closes_quote_run(line, quotes) {
                    self.region = (!line_closes_attribute(line)).then_some(AttrRegion::Open(attr));
                }
                RefContext::AttributeString
            }
            Some(AttrRegion::Open(attr)) => self.continue_attribute(attr, line),
            None => {
                let opened = if line_opens_type_attr(line) {
                    RefContext::TypeAttribute
                } else if line_opens_other_attr(line) {
                    RefContext::OtherAttribute
                } else {
                    return RefContext::Body;
                };
                self.continue_attribute(opened, line)
            }
        }
    }

    /// Advance an attribute region across `line`: a trailing quote run
    /// opens a string, a terminating `.` closes the region, and
    /// anything else carries it to the next line.
    fn continue_attribute(&mut self, attr: RefContext, line: &str) -> RefContext {
        self.region = match trailing_quote_run(line) {
            Some(quotes) => Some(AttrRegion::String { attr, quotes }),
            None if line_closes_attribute(line) => None,
            None => Some(AttrRegion::Open(attr)),
        };
        attr
    }
}

/// The length of a trailing run of three or more `"`, which opens a
/// triple-quoted string that the rest of the line does not close. The
/// rule is name-independent: such a string can open inside any
/// attribute, not only the documentation forms.
fn trailing_quote_run(line: &str) -> Option<usize> {
    let run = strip_line_comment(line)
        .trim_end()
        .bytes()
        .rev()
        .take_while(|b| *b == b'"')
        .count();
    (run >= 3).then_some(run)
}

/// Whether `line` starts, after leading whitespace, with a run of at
/// least `open` quotes: the EEP-64 closing form.
fn line_closes_quote_run(line: &str, open: usize) -> bool {
    line.trim_start().bytes().take_while(|b| *b == b'"').count() >= open
}

fn line_opens_type_attr(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('-') else {
        return false;
    };
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
        .unwrap_or(rest.len());
    if !opens_type_ref_context(&rest[..name_end]) {
        return false;
    }
    matches!(rest[name_end..].chars().next(), Some(c) if c == '(' || c.is_whitespace())
}

/// A line-leading `-` followed by a lowercase letter opens an
/// attribute form; `-1` or `- X` on a wrapped expression line does not.
fn line_opens_other_attr(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    chars.next() == Some('-') && chars.next().is_some_and(|c| c.is_ascii_lowercase())
}

fn line_closes_attribute(line: &str) -> bool {
    let stripped = strip_line_comment(line);
    stripped.trim_end().ends_with('.')
}

/// Cuts an unquoted `%` comment off a source line. Strings, quoted
/// atoms, escapes, and `$c` char literals (`$%`, `$"`) stay intact.
pub fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut in_atom = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str || in_atom {
            match b {
                b'\\' => i += 1,
                b'"' if in_str => in_str = false,
                b'\'' if in_atom => in_atom = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'\'' => in_atom = true,
            b'$' => {
                i += skip_char_literal_span(bytes, i);
                continue;
            }
            b'%' => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

/// Type-attribute variant of `extract_into_with_macros`.
///
/// Inside `-spec`, `-callback`, `-type`, and `-opaque` declarations the
/// `mod:ident(...)` shape is a type reference (`ra:index()` is the
/// type `index/0`, not a call). Records and apply-family BIFs are
/// syntactically impossible there, so this extractor only emits type
/// refs and macro uses. Macro values are not resolved into MFAs in
/// this context: a `?MOD:fn` expansion in a spec position would be a
/// type, but the macro tables today carry value-to-atom bindings
/// only, with no type-side equivalent.
pub fn extract_type_refs_into(line: &str, out: &mut Vec<SymbolRef>) {
    let line = strip_line_comment(line);
    let mut with_offsets = Vec::new();
    push_type_refs_with_offsets(line, &mut with_offsets);
    out.extend(with_offsets.into_iter().map(|(r, _)| r));
}

/// `extract_type_refs_into` over already-stripped text, with each
/// reference's match offset for origin mapping.
fn push_type_refs_with_offsets(text: &str, out: &mut Vec<(SymbolRef, usize)>) {
    for caps in call_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        let module = &caps[1];
        let ident = &caps[2];
        // There is no any-arity type kind, so a type ref that does not
        // close even across the run is skipped, not guessed.
        let ScanArity::Exact(arity) = scan_arity(&text[group.end()..]) else {
            continue;
        };
        if let (Ok(m), Ok(t)) = (ModuleName::from_str(module), TypeName::from_str(ident)) {
            out.push((SymbolRef::type_ref(m, t, Arity::new(arity)), group.start()));
        }
    }
    for caps in macro_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        out.push((SymbolRef::macro_use(caps[1].to_owned()), group.start()));
    }
}

fn empty_macros() -> &'static MacroTable {
    static EMPTY: OnceLock<MacroTable> = OnceLock::new();
    EMPTY.get_or_init(MacroTable::new)
}

/// Same as `extract_into` but consults `macros` so `?Name`-style
/// references resolve into concrete `mod:fn(...)` symbols when the
/// macro's body is a bare atom (module) or `mod:fn` pair. Apply-family
/// BIFs whose `M`, `F`, and arg-list slots are literals also resolve
/// into the matching `Mfa` and are added to `out` as normal function refs.
pub fn extract_into_with_macros(line: &str, macros: &MacroTable, out: &mut Vec<SymbolRef>) {
    let line = strip_line_comment(line);
    let mut with_offsets = Vec::new();
    push_refs_with_offsets(line, macros, &mut with_offsets);
    out.extend(with_offsets.into_iter().map(|(r, _)| r));
}

/// `extract_into_with_macros` over already-stripped text, pairing each
/// reference with the byte offset its match started at, so the caller
/// can map it to the origin of the physical line that offset lands on.
fn push_refs_with_offsets(text: &str, macros: &MacroTable, out: &mut Vec<(SymbolRef, usize)>) {
    for caps in call_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        let module = &caps[1];
        let function = &caps[2];
        let after = &text[group.end()..];
        let (Ok(m), Ok(f)) = (
            ModuleName::from_str(module),
            FunctionName::from_str(function),
        ) else {
            continue;
        };
        out.push((scanned_call(scan_arity(after), m, f), group.start()));
    }
    push_fun_refs_with_offsets(text, macros, out);
    for caps in record_re().captures_iter(text) {
        let start = caps.get(0).expect("capture").start();
        // a based literal's `#` is not a record reference
        if hash_inside_number(text.as_bytes(), start) {
            continue;
        }
        if let Ok(name) = RecordName::from_str(&caps[1]) {
            out.push((SymbolRef::record(name), start));
        }
    }
    for caps in macro_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        let name = &caps[1];
        out.push((SymbolRef::macro_use(name.to_owned()), group.start()));
        let mut resolved = Vec::new();
        try_resolve_macro_call(name, &text[group.end()..], macros, &mut resolved);
        out.extend(resolved.into_iter().map(|r| (r, group.start())));
    }
    push_apply_family_with_offsets(text, macros, out);
}

fn fun_ref_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"\bfun\s+(\??[A-Za-z_][a-zA-Z0-9_@]*)\s*:\s*([A-Za-z_][a-zA-Z0-9_@]*)\s*/\s*([0-9]+|[A-Za-z_][a-zA-Z0-9_@]*)",
        )
        .expect("regex")
    })
}

/// `fun M:F/A` remote fun references. The arity is written in the
/// source, so it is exact. Variable-shaped slots are classified as
/// dynamic calls instead.
fn push_fun_refs_with_offsets(text: &str, macros: &MacroTable, out: &mut Vec<(SymbolRef, usize)>) {
    for caps in fun_ref_re().captures_iter(text) {
        let module_raw = &caps[1];
        let function_raw = &caps[2];
        let arity_raw = &caps[3];
        let Ok(arity) = arity_raw.parse::<u8>() else {
            continue;
        };
        if !function_raw.starts_with(|c: char| c.is_ascii_lowercase()) {
            continue;
        }
        let module = if let Some(macro_name) = module_raw.strip_prefix('?') {
            let Some(expanded) = expand_value_macro_to_atom(macros, macro_name) else {
                continue;
            };
            let Ok(m) = ModuleName::from_str(&expanded) else {
                continue;
            };
            m
        } else if module_raw.starts_with(|c: char| c.is_ascii_lowercase()) {
            let Ok(m) = ModuleName::from_str(module_raw) else {
                continue;
            };
            m
        } else {
            continue;
        };
        let Ok(f) = FunctionName::from_str(function_raw) else {
            continue;
        };
        let start = caps.get(0).expect("capture").start();
        out.push((
            SymbolRef::function(Mfa::new(module, f, Arity::new(arity))),
            start,
        ));
    }
}

fn try_resolve_macro_call(name: &str, tail: &str, macros: &MacroTable, out: &mut Vec<SymbolRef>) {
    let trimmed = tail.trim_start();
    if let Some(rest) = trimmed.strip_prefix('(') {
        let Some((m, f)) = expand_value_macro_to_mf(macros, name) else {
            return;
        };
        let (Ok(mn), Ok(fn_)) = (ModuleName::from_str(&m), FunctionName::from_str(&f)) else {
            return;
        };
        out.push(scanned_call(scan_arity(rest), mn, fn_));
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
                    out.push(scanned_call(scan_arity(after_open), mn, fn_));
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

fn push_apply_family_with_offsets(
    text: &str,
    macros: &MacroTable,
    out: &mut Vec<(SymbolRef, usize)>,
) {
    for caps in apply_family_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        let name = &caps[1];
        let after_open = &text[group.end()..];
        // resolving from a wrapped call's partial slot set would fabricate an Mfa
        let ScannedArgs::Terminated { args, .. } = scan_top_level_args(after_open) else {
            continue;
        };
        if let Some(mfa) = resolve_apply_family(name, &args, macros) {
            out.push((SymbolRef::function(mfa), group.start()));
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
    let module = atom_or_macro_to::<ModuleName>(m_raw, macros)?;
    let function = atom_or_macro_to::<FunctionName>(f_raw, macros)?;
    let arity = literal_list_length(a_raw)?;
    Some(Mfa::new(module, function, Arity::new(arity)))
}

// ModuleName and FunctionName are both atoms: one resolver covers both
fn atom_or_macro_to<T: FromStr>(raw: &str, macros: &MacroTable) -> Option<T> {
    let s = raw.trim();
    if let Some(name) = s.strip_prefix('?') {
        return expand_value_macro_to_atom(macros, name).and_then(|a| T::from_str(&a).ok());
    }
    if is_bare_atom(s) {
        return T::from_str(s).ok();
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return T::from_str(&s[1..s.len() - 1]).ok();
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

/// Collects per-call argument shapes from one source line. Mirrors
/// `extract_into` for the `mod:fun(args)` case; classifies each actual
/// into an `ArgShape` so the analyzer can match against pin-side
/// clause-head patterns.
pub fn extract_call_args_into(line: &str, out: &mut Vec<(Mfa, Vec<ArgShape>)>) {
    let line = strip_line_comment(line);
    let mut with_offsets = Vec::new();
    push_call_args_with_offsets(line, &mut with_offsets);
    out.extend(with_offsets.into_iter().map(|(c, _)| c));
}

/// `extract_call_args_into` over already-stripped text, with each call's
/// match offset so the caller can keep only `Added`-origin calls.
fn push_call_args_with_offsets(text: &str, out: &mut Vec<((Mfa, Vec<ArgShape>), usize)>) {
    for caps in call_re().captures_iter(text) {
        let group = caps.get(0).expect("capture");
        let module = &caps[1];
        let function = &caps[2];
        // an unterminated arg list has no complete shape list to compare
        let Some((args, _consumed)) = extract_arg_shapes(&text[group.end()..]) else {
            continue;
        };
        let arity = u8::try_from(args.len()).unwrap_or(u8::MAX);
        if let (Ok(m), Ok(f)) = (
            ModuleName::from_str(module),
            FunctionName::from_str(function),
        ) {
            out.push(((Mfa::new(m, f, Arity::new(arity)), args), group.start()));
        }
    }
}

fn extract_arg_shapes(after_open_paren: &str) -> Option<(Vec<ArgShape>, usize)> {
    match scan_top_level_args(after_open_paren) {
        ScannedArgs::Terminated { args, consumed } => {
            Some((args.iter().map(|a| classify_arg(a)).collect(), consumed))
        }
        ScannedArgs::Unterminated { .. } => None,
    }
}

pub fn classify_arg(raw: &str) -> ArgShape {
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

fn is_bare_identifier(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

pub fn extract_dynamic_into(line: &str, out: &mut Vec<DynamicCall>) {
    let line = strip_line_comment(line);
    for _ in apply_bif_re().captures_iter(line) {
        out.push(DynamicCall::Apply);
    }
    for _ in var_module_call_re().find_iter(line) {
        out.push(DynamicCall::VariableDispatch);
    }
    for _ in var_function_call_re().find_iter(line) {
        out.push(DynamicCall::VariableDispatch);
    }
    for caps in fun_ref_re().captures_iter(line) {
        let variable_slot = |s: &str| s.starts_with(|c: char| c.is_ascii_uppercase() || c == '_');
        let arity_raw = &caps[3];
        let variable_arity = !arity_raw.bytes().all(|b| b.is_ascii_digit());
        if variable_slot(&caps[1]) || variable_slot(&caps[2]) || variable_arity {
            out.push(DynamicCall::VariableDispatch);
        }
    }
}

pub fn extract_definitions_into(text: &str, out: &mut Vec<SymbolRef>) {
    for caps in fun_def_re().captures_iter(text) {
        let head_end = caps.get(0).expect("capture").end();
        let after = &text[head_end..];
        let Ok(f) = FunctionName::from_str(&caps[1]) else {
            continue;
        };
        let module = local_placeholder_module().clone();
        match scan_arity(after) {
            ScanArity::Exact(arity) => {
                out.push(SymbolRef::function(Mfa::new(module, f, Arity::new(arity))));
            }
            ScanArity::Unterminated => {
                out.push(SymbolRef::function_any_arity(module, f));
            }
        }
    }
}

fn local_placeholder_module() -> &'static ModuleName {
    static M: OnceLock<ModuleName> = OnceLock::new();
    M.get_or_init(|| ModuleName::from_str("_local").expect("valid"))
}

/// Origin-tagged references, definitions, dynamic calls, and `Added`
/// call-arg shapes pulled from one Erlang hunk.
#[derive(Debug, Default)]
pub struct HunkScan {
    pub referenced: Vec<SymbolRef>,
    pub defined: Vec<SymbolRef>,
    pub dynamic_calls: Vec<DynamicCall>,
    pub call_args: Vec<(Mfa, Vec<ArgShape>)>,
}

/// Scan one Erlang hunk's lines as logical units, not one diff line at a
/// time. `lines` is the hunk's `Added` and `Context` lines in order,
/// `Removed` already dropped. Consecutive lines of the same context
/// (`Body` or `TypeAttribute`) are joined and scanned together, so a
/// call or clause head whose argument list wraps across lines resolves
/// at exact arity. Each reference takes the origin of the physical line
/// its match started on.
pub fn scan_hunk(lines: &[(RefOrigin, &str)], macros: &MacroTable) -> HunkScan {
    let mut out = HunkScan::default();
    let mut scanner = AttrCtxScanner::new();
    let mut text = String::new();
    let mut line_origins: Vec<(usize, RefOrigin)> = Vec::new();
    // runs group on the type-attribute boundary alone, so other-attribute
    // lines join body runs exactly as they did before the third variant
    let mut run_ctx: Option<bool> = None;
    for &(origin, raw) in lines {
        let stripped = strip_line_comment(raw);
        let ctx = scanner.classify(stripped);
        // a multi-line string in an attribute holds no references at all,
        // and joining its prose to the lines around it would build one
        // construct out of two unrelated halves
        if ctx == RefContext::AttributeString {
            if let Some(prev) = run_ctx.take() {
                flush_run(prev, &text, &line_origins, macros, &mut out);
                text.clear();
                line_origins.clear();
            }
            continue;
        }
        let type_attr = ctx == RefContext::TypeAttribute;
        if run_ctx != Some(type_attr) {
            if let Some(prev) = run_ctx {
                flush_run(prev, &text, &line_origins, macros, &mut out);
            }
            text.clear();
            line_origins.clear();
            run_ctx = Some(type_attr);
        }
        line_origins.push((text.len(), origin));
        text.push_str(stripped);
        text.push('\n');
    }
    if let Some(prev) = run_ctx {
        flush_run(prev, &text, &line_origins, macros, &mut out);
    }
    out
}

fn flush_run(
    type_attr: bool,
    text: &str,
    line_origins: &[(usize, RefOrigin)],
    macros: &MacroTable,
    out: &mut HunkScan,
) {
    if type_attr {
        let mut refs = Vec::new();
        push_type_refs_with_offsets(text, &mut refs);
        tag_by_origin(refs, line_origins, &mut out.referenced);
        return;
    }
    let mut refs = Vec::new();
    push_refs_with_offsets(text, macros, &mut refs);
    tag_by_origin(refs, line_origins, &mut out.referenced);
    extract_definitions_into(text, &mut out.defined);
    extract_dynamic_into(text, &mut out.dynamic_calls);
    let mut args = Vec::new();
    push_call_args_with_offsets(text, &mut args);
    // context-line calls are pre-existing target facts, not clause-mismatch input
    for (call, offset) in args {
        if origin_at(line_origins, offset) == RefOrigin::Added {
            out.call_args.push(call);
        }
    }
}

fn tag_by_origin(
    refs: Vec<(SymbolRef, usize)>,
    line_origins: &[(usize, RefOrigin)],
    out: &mut Vec<SymbolRef>,
) {
    for (r, offset) in refs {
        out.push(r.with_origin(origin_at(line_origins, offset)));
    }
}

/// The origin of the physical line a match offset lands on: the last
/// line whose start is at or before the offset, else `Context`.
fn origin_at(line_origins: &[(usize, RefOrigin)], offset: usize) -> RefOrigin {
    value_at_or_before(line_origins, offset, RefOrigin::Context)
}
