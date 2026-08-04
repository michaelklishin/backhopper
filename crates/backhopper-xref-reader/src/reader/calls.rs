// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Call-site detection: tries each of `?MODULE:f(...)`, `mod:fun(...)`,
//! variable-module or variable-function variants, then bare local calls.
//! Walks recursively into argument lists so nested call sites are surfaced
//! too: `a:f(b:g())` records both `a:f/1` and `b:g/0`.

use backhopper_core::{Arity, FunctionName, Mfa, ModuleName};
use backhopper_xref_graph::{
    CallKind, CallTarget, FunctionRef, FunctionSig, Loc, LocalFunctionRef, Position,
    UnresolvedCallSite,
};

use crate::macros::{MacroKey, expand_value_macro_to_atom, expand_value_macro_to_mf};
use crate::model::CallSite;
use crate::reader::scan::{ModuleBuilder, span_loc};
use crate::scanner::{Scanner, walk_paren_group};

const APPLY_FAMILY: &[&str] = &[
    "apply",
    "spawn",
    "spawn_link",
    "spawn_monitor",
    "spawn_opt",
    "hibernate",
];

const KEYWORDS: &[&str] = &[
    "case", "if", "receive", "try", "begin", "when", "of", "end", "fun", "after", "catch", "and",
    "or", "andalso", "orelse", "not", "div", "rem", "band", "bor", "bxor", "bnot", "bsl", "bsr",
    "xor",
];

pub(super) fn try_function_clause(
    sc: &mut Scanner<'_>,
    clause_pos: Position,
    b: &mut ModuleBuilder,
) -> Option<FunctionSig> {
    let save_pos = sc.pos();
    let name_str = sc.consume_identifier();
    if name_str.is_empty() {
        return None;
    }
    sc.skip_trivia();
    if sc.peek() != Some(b'(') {
        sc.set_pos(save_pos);
        return None;
    }
    let name = match FunctionName::new(name_str.to_owned()) {
        Ok(n) => n,
        Err(_) => {
            sc.set_pos(save_pos);
            return None;
        }
    };
    let arity = arity_from_parenthesised_args(sc);
    sc.skip_trivia();
    let sig = FunctionSig::new(name, Arity::new(arity));
    let def_loc = Loc::point(b.path_id, clause_pos);
    b.definitions.entry(sig.clone()).or_insert(def_loc);
    Some(sig)
}

/// Counts top-level commas inside `(...)` and returns the arity. Used
/// only for function-clause heads, where we deliberately don't recurse
/// into argument patterns.
pub(super) fn arity_from_parenthesised_args(sc: &mut Scanner<'_>) -> u8 {
    if sc.peek() != Some(b'(') {
        return 0;
    }
    let w = walk_paren_group(sc, |_| false);
    arity_from(w.commas, w.had_content)
}

/// Like `arity_from_parenthesised_args`, but also runs `try_call_site` at
/// every identifier-starting byte inside the parens so nested call sites
/// are surfaced.
fn scan_args_for_calls(sc: &mut Scanner<'_>, caller: &FunctionSig, b: &mut ModuleBuilder) -> u8 {
    if sc.peek() != Some(b'(') {
        return 0;
    }
    let w = walk_paren_group(sc, |sc| {
        let pos = sc.pos();
        try_call_site(sc, pos, caller, b)
    });
    arity_from(w.commas, w.had_content)
}

fn arity_from(commas: u32, had_content: bool) -> u8 {
    let raw = if had_content { commas + 1 } else { 0 };
    u8::try_from(raw).unwrap_or(u8::MAX)
}

pub(super) fn try_call_site(
    sc: &mut Scanner<'_>,
    pos: Position,
    caller: &FunctionSig,
    b: &mut ModuleBuilder,
) -> bool {
    let byte = sc.peek();
    if byte == Some(b'?') {
        return try_macro_use(sc, pos, caller, b);
    }

    if matches!(byte, Some(b'a'..=b'z') | Some(b'_')) {
        let save = sc.pos();
        let head = sc.consume_identifier();
        if sc.peek() == Some(b':') {
            let next = sc.peek_at(1);
            if matches!(next, Some(b'a'..=b'z')) {
                sc.advance();
                let f = sc.consume_identifier();
                sc.skip_trivia();
                if sc.peek() == Some(b'(') {
                    if head == "erlang" && APPLY_FAMILY.contains(&f) {
                        return record_apply_family_call(sc, f, caller, b, pos);
                    }
                    let arity = scan_args_for_calls(sc, caller, b);
                    record_external_call(b, caller, head, f, arity, pos, sc.pos());
                    return true;
                } else if sc.peek() == Some(b'/') {
                    sc.advance();
                    let arity = consume_integer(sc);
                    record_fun_ref_external(b, caller, head, f, arity, pos, sc.pos());
                    return true;
                }
                record_fun_ref_external(b, caller, head, f, 0, pos, sc.pos());
                return true;
            }
            if matches!(next, Some(b'A'..=b'Z')) {
                sc.advance();
                let _f = sc.consume_identifier();
                sc.skip_trivia();
                let arity = if sc.peek() == Some(b'(') {
                    Some(scan_args_for_calls(sc, caller, b))
                } else {
                    None
                };
                let module = ModuleName::new(head.to_owned()).ok();
                if let (Some(m), Some(caller_mfa)) = (module, caller_mfa(b, caller)) {
                    b.unresolved.insert(UnresolvedCallSite {
                        caller: caller_mfa,
                        partial: FunctionRef::UnresolvedFunction {
                            module: m,
                            arity: arity.map(Arity::new),
                        },
                        kind: CallKind::Direct,
                        loc: span_loc(b, pos, sc.pos()),
                    });
                }
                return true;
            }
        }
        sc.set_pos(save);
    }

    if matches!(byte, Some(b'A'..=b'Z')) {
        let save = sc.pos();
        let _var = sc.consume_identifier();
        if sc.peek() == Some(b':') {
            let next = sc.peek_at(1);
            if matches!(next, Some(b'a'..=b'z')) {
                sc.advance();
                let f = sc.consume_identifier();
                sc.skip_trivia();
                let arity = if sc.peek() == Some(b'(') {
                    Some(scan_args_for_calls(sc, caller, b))
                } else {
                    None
                };
                if let (Ok(name), Some(caller_mfa)) =
                    (FunctionName::new(f.to_owned()), caller_mfa(b, caller))
                {
                    b.unresolved.insert(UnresolvedCallSite {
                        caller: caller_mfa,
                        partial: FunctionRef::UnresolvedModule {
                            function: name,
                            arity: arity.map(Arity::new),
                        },
                        kind: CallKind::Direct,
                        loc: span_loc(b, pos, sc.pos()),
                    });
                }
                return true;
            }
        }
        sc.set_pos(save);
    }

    if matches!(byte, Some(b'a'..=b'z')) {
        let save = sc.pos();
        let f = sc.consume_identifier();
        sc.skip_trivia();
        if sc.peek() == Some(b'(') {
            if KEYWORDS.contains(&f) {
                sc.set_pos(save);
                return false;
            }
            if APPLY_FAMILY.contains(&f) {
                return record_apply_family_call(sc, f, caller, b, pos);
            }
            let arity = scan_args_for_calls(sc, caller, b);
            record_local_call(b, caller, f, arity, pos, sc.pos());
            return true;
        }
        sc.set_pos(save);
    }

    false
}

/// `?Name` and `?Name(args)` and `?Name:f(args)`. Looks `Name` up in the
/// module's macro table (filled by `-define`s and resolved `-include`s).
/// `?MODULE` and `?MODULE:f(args)` are the legacy short-circuit that
/// predates the table.
fn try_macro_use(
    sc: &mut Scanner<'_>,
    pos: Position,
    caller: &FunctionSig,
    b: &mut ModuleBuilder,
) -> bool {
    sc.advance();
    let name = sc.consume_identifier().to_owned();
    if name.is_empty() {
        return true;
    }

    if name == "MODULE" {
        if sc.peek() == Some(b':') && matches!(sc.peek_at(1), Some(b'a'..=b'z')) {
            sc.advance();
            let f = sc.consume_identifier().to_owned();
            sc.skip_trivia();
            if sc.peek() == Some(b'(') {
                let arity = scan_args_for_calls(sc, caller, b);
                record_call_self_module(b, caller, &f, arity, pos, sc.pos());
                return true;
            }
        }
        return true;
    }

    if sc.peek() == Some(b'(') {
        if let Some(arity) = peek_arg_count(sc) {
            let key = MacroKey {
                name: name.clone(),
                arity: Some(arity),
            };
            if let Some(body) = b.macros.get(&key).cloned() {
                scan_macro_body_calls(b, &body, caller, pos);
                scan_args_for_calls(sc, caller, b);
                return true;
            }
        }
        if let Some((module, function)) = expand_value_macro_to_mf(&b.macros, &name) {
            let arity = scan_args_for_calls(sc, caller, b);
            record_external_call(b, caller, &module, &function, arity, pos, sc.pos());
            return true;
        }
    }

    if sc.peek() == Some(b':') && matches!(sc.peek_at(1), Some(b'a'..=b'z')) {
        if let Some(module) = expand_value_macro_to_atom(&b.macros, &name) {
            sc.advance();
            let f = sc.consume_identifier().to_owned();
            sc.skip_trivia();
            if sc.peek() == Some(b'(') {
                let arity = scan_args_for_calls(sc, caller, b);
                record_external_call(b, caller, &module, &f, arity, pos, sc.pos());
                return true;
            }
        }
    }

    if let Some(body) = b
        .macros
        .get(&MacroKey {
            name: name.clone(),
            arity: None,
        })
        .cloned()
    {
        scan_macro_body_calls(b, &body, caller, pos);
        return true;
    }

    true
}

/// Returns the number of top-level comma-separated arguments inside the
/// `(...)` group at the current cursor without moving the cursor.
fn peek_arg_count(sc: &Scanner<'_>) -> Option<u8> {
    let mut cur = sc.clone();
    if cur.peek() != Some(b'(') {
        return None;
    }
    let w = walk_paren_group(&mut cur, |_| false);
    if !w.closed {
        return None;
    }
    Some(arity_from(w.commas, w.had_content))
}

/// Scans a macro body for any call sites and records them at the
/// use-site `Loc`. Skips parameter substitution: callers we'd otherwise
/// have resolved by substituting params land as `Unresolved*` instead.
fn scan_macro_body_calls(
    b: &mut ModuleBuilder,
    body: &str,
    caller: &FunctionSig,
    use_site: Position,
) {
    let mut sc = Scanner::new(body);
    while !sc.is_eof() {
        let Some(byte) = sc.peek() else { break };
        match byte {
            b'"' => sc.consume_string(),
            b'~' => sc.consume_sigil_or_advance(),
            b'\'' => sc.consume_quoted_atom(),
            b'$' => sc.consume_char_literal(),
            b'%' => sc.skip_line(),
            b'a'..=b'z' | b'_' => {
                if !try_call_in_macro_body(&mut sc, caller, b, use_site) {
                    sc.advance();
                }
            }
            _ => {
                sc.advance();
            }
        }
    }
}

fn try_call_in_macro_body(
    sc: &mut Scanner<'_>,
    caller: &FunctionSig,
    b: &mut ModuleBuilder,
    use_site: Position,
) -> bool {
    let save = sc.pos();
    let head = sc.consume_identifier().to_owned();
    if sc.peek() == Some(b':') && matches!(sc.peek_at(1), Some(b'a'..=b'z')) {
        sc.advance();
        let f = sc.consume_identifier().to_owned();
        sc.skip_trivia();
        if sc.peek() == Some(b'(') {
            let arity = arity_from_parenthesised_args(sc);
            record_external_call(b, caller, &head, &f, arity, use_site, use_site);
            return true;
        }
    }
    sc.skip_trivia();
    if sc.peek() == Some(b'(') && !KEYWORDS.contains(&head.as_str()) {
        let arity = arity_from_parenthesised_args(sc);
        record_local_call(b, caller, &head, arity, use_site, use_site);
        return true;
    }
    sc.set_pos(save);
    false
}

/// `apply(M, F, A)`, `spawn(M, F, A)`, `spawn(Node, M, F, A)`, and the
/// rest of the apply-family. Returns true after recording a concrete or
/// unresolved call site.
fn record_apply_family_call(
    sc: &mut Scanner<'_>,
    name: &str,
    caller: &FunctionSig,
    b: &mut ModuleBuilder,
    pos: Position,
) -> bool {
    let kind = if name == "apply" {
        CallKind::Apply
    } else {
        CallKind::Spawn
    };
    let args = parse_literal_args(sc, caller, b);
    let resolved = resolve_apply_family(name, &args);
    let end = sc.pos();
    match resolved {
        ApplyResolution::Concrete {
            module,
            function,
            arity,
        } => {
            let mfa = Mfa::new(module, function, Arity::new(arity));
            b.external_calls.push(CallSite {
                caller: caller.clone(),
                callee: CallTarget::External(FunctionRef::Concrete(mfa)),
                kind,
                loc: span_loc(b, pos, end),
            });
        }
        ApplyResolution::Unresolved { partial } => {
            if let Some(caller_mfa) = caller_mfa(b, caller) {
                b.unresolved.insert(UnresolvedCallSite {
                    caller: caller_mfa,
                    partial,
                    kind,
                    loc: span_loc(b, pos, end),
                });
            }
        }
    }
    true
}

#[derive(Debug)]
enum LiteralArg {
    Atom(String),
    List { length: Option<u8> },
    Other,
}

enum ApplyResolution {
    Concrete {
        module: ModuleName,
        function: FunctionName,
        arity: u8,
    },
    Unresolved {
        partial: FunctionRef,
    },
}

/// Captures each top-level argument's source text and runs `try_call_site`
/// inside so nested calls in the args are still surfaced.
fn parse_literal_args(
    sc: &mut Scanner<'_>,
    caller: &FunctionSig,
    b: &mut ModuleBuilder,
) -> Vec<LiteralArg> {
    if sc.peek() != Some(b'(') {
        return Vec::new();
    }
    sc.advance();
    let mut args: Vec<LiteralArg> = Vec::new();
    let mut arg_start = sc.pos().byte_offset as usize;
    let mut depth = 1i32;
    while let Some(byte) = sc.peek() {
        match byte {
            b'(' => {
                depth += 1;
                sc.advance();
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let end = sc.pos().byte_offset as usize;
                    let slice = sc.src_slice(arg_start, end);
                    if !slice.trim().is_empty() {
                        args.push(classify_literal_arg(slice));
                    }
                    sc.advance();
                    return args;
                }
                sc.advance();
            }
            b'[' | b'{' => {
                depth += 1;
                sc.advance();
            }
            b']' | b'}' => {
                depth -= 1;
                sc.advance();
            }
            // << and >> nest like brackets so a binary literal is not split into separate arguments.
            b'<' if sc.peek_at(1) == Some(b'<') => {
                depth += 1;
                sc.advance();
                sc.advance();
            }
            b'>' if sc.peek_at(1) == Some(b'>') => {
                depth -= 1;
                sc.advance();
                sc.advance();
            }
            b',' if depth == 1 => {
                let end = sc.pos().byte_offset as usize;
                let slice = sc.src_slice(arg_start, end);
                args.push(classify_literal_arg(slice));
                sc.advance();
                arg_start = sc.pos().byte_offset as usize;
            }
            b'"' => sc.consume_string(),
            b'~' => sc.consume_sigil_or_advance(),
            b'\'' => sc.consume_quoted_atom(),
            b'$' => sc.consume_char_literal(),
            b'%' => sc.skip_line(),
            b'?' | b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let scoped_pos = sc.pos();
                if !try_call_site(sc, scoped_pos, caller, b) {
                    sc.advance();
                }
            }
            _ => {
                sc.advance();
            }
        }
    }
    args
}

fn classify_literal_arg(slice: &str) -> LiteralArg {
    let trimmed = slice.trim();
    if trimmed.is_empty() {
        return LiteralArg::Other;
    }
    if looks_like_lowercase_atom(trimmed) {
        return LiteralArg::Atom(trimmed.to_owned());
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return LiteralArg::Atom(inner.to_owned());
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let trimmed_inner = inner.trim();
        if trimmed_inner.is_empty() {
            return LiteralArg::List { length: Some(0) };
        }
        let length = backhopper_erlang_scan::count_top_level_commas(trimmed_inner) + 1;
        return LiteralArg::List {
            length: u8::try_from(length).ok(),
        };
    }
    LiteralArg::Other
}

fn looks_like_lowercase_atom(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
}

fn resolve_apply_family(name: &str, args: &[LiteralArg]) -> ApplyResolution {
    let mfa = match (name, args.len()) {
        ("apply", 3) => Some((&args[0], &args[1], &args[2])),
        ("spawn" | "spawn_link" | "spawn_monitor" | "hibernate", 3) => {
            Some((&args[0], &args[1], &args[2]))
        }
        ("spawn" | "spawn_link" | "spawn_monitor", 4) => Some((&args[1], &args[2], &args[3])),
        ("spawn_opt", 4) => Some((&args[0], &args[1], &args[2])),
        ("spawn_opt", 5) => Some((&args[1], &args[2], &args[3])),
        _ => None,
    };
    let Some((m_arg, f_arg, a_arg)) = mfa else {
        return ApplyResolution::Unresolved {
            partial: FunctionRef::UnresolvedBoth { arity: None },
        };
    };
    let module = atom_to_module(m_arg);
    let function = atom_to_function(f_arg);
    let arity = list_length(a_arg);
    match (module, function, arity) {
        (Some(m), Some(f), Some(a)) => ApplyResolution::Concrete {
            module: m,
            function: f,
            arity: a,
        },
        (m_opt, f_opt, a_opt) => ApplyResolution::Unresolved {
            partial: build_partial(m_opt, f_opt, a_opt),
        },
    }
}

fn atom_to_module(arg: &LiteralArg) -> Option<ModuleName> {
    match arg {
        LiteralArg::Atom(s) => ModuleName::new(s.clone()).ok(),
        _ => None,
    }
}

fn atom_to_function(arg: &LiteralArg) -> Option<FunctionName> {
    match arg {
        LiteralArg::Atom(s) => FunctionName::new(s.clone()).ok(),
        _ => None,
    }
}

fn list_length(arg: &LiteralArg) -> Option<u8> {
    match arg {
        LiteralArg::List { length } => *length,
        _ => None,
    }
}

fn build_partial(
    module: Option<ModuleName>,
    function: Option<FunctionName>,
    arity: Option<u8>,
) -> FunctionRef {
    let arity_opt = arity.map(Arity::new);
    match (module, function) {
        (Some(m), Some(f)) => {
            FunctionRef::Concrete(Mfa::new(m, f, arity_opt.unwrap_or_else(|| Arity::new(0))))
        }
        (Some(m), None) => FunctionRef::UnresolvedFunction {
            module: m,
            arity: arity_opt,
        },
        (None, Some(f)) => FunctionRef::UnresolvedModule {
            function: f,
            arity: arity_opt,
        },
        (None, None) => FunctionRef::UnresolvedBoth { arity: arity_opt },
    }
}

fn consume_integer(sc: &mut Scanner<'_>) -> u8 {
    let start = sc.pos().byte_offset as usize;
    while let Some(b) = sc.peek() {
        if b.is_ascii_digit() {
            sc.advance();
        } else {
            break;
        }
    }
    let end = sc.pos().byte_offset as usize;
    sc.src_slice(start, end).parse::<u8>().unwrap_or(0)
}

fn record_local_call(
    b: &mut ModuleBuilder,
    caller: &FunctionSig,
    callee_name: &str,
    arity: u8,
    start: Position,
    end: Position,
) {
    if b.name.is_none() {
        return;
    }
    let Ok(callee) = FunctionName::new(callee_name.to_owned()) else {
        return;
    };
    let sig = FunctionSig::new(callee.clone(), Arity::new(arity));
    let import_module = b
        .imports
        .iter()
        .find_map(|(m, sigs)| sigs.contains(&sig).then(|| m.clone()));
    if let Some(import_mod) = import_module {
        let callee_mfa = Mfa::new(import_mod, callee, Arity::new(arity));
        b.external_calls.push(CallSite {
            caller: caller.clone(),
            callee: CallTarget::External(FunctionRef::Concrete(callee_mfa)),
            kind: CallKind::Direct,
            loc: span_loc(b, start, end),
        });
        return;
    }
    b.local_calls.push(CallSite {
        caller: caller.clone(),
        callee: CallTarget::Local(LocalFunctionRef::Concrete {
            function: callee,
            arity: Arity::new(arity),
        }),
        kind: CallKind::Direct,
        loc: span_loc(b, start, end),
    });
}

fn record_external_call(
    b: &mut ModuleBuilder,
    caller: &FunctionSig,
    module: &str,
    function: &str,
    arity: u8,
    start: Position,
    end: Position,
) {
    let (Ok(m), Ok(f)) = (
        ModuleName::new(module.to_owned()),
        FunctionName::new(function.to_owned()),
    ) else {
        return;
    };
    let mfa = Mfa::new(m, f, Arity::new(arity));
    b.external_calls.push(CallSite {
        caller: caller.clone(),
        callee: CallTarget::External(FunctionRef::Concrete(mfa)),
        kind: CallKind::Direct,
        loc: span_loc(b, start, end),
    });
}

fn record_call_self_module(
    b: &mut ModuleBuilder,
    caller: &FunctionSig,
    function: &str,
    arity: u8,
    start: Position,
    end: Position,
) {
    let Some(module) = b.name.clone() else { return };
    let Ok(f) = FunctionName::new(function.to_owned()) else {
        return;
    };
    let mfa = Mfa::new(module, f, Arity::new(arity));
    b.external_calls.push(CallSite {
        caller: caller.clone(),
        callee: CallTarget::External(FunctionRef::Concrete(mfa)),
        kind: CallKind::Direct,
        loc: span_loc(b, start, end),
    });
}

fn record_fun_ref_external(
    b: &mut ModuleBuilder,
    caller: &FunctionSig,
    module: &str,
    function: &str,
    arity: u8,
    start: Position,
    end: Position,
) {
    let (Ok(m), Ok(f)) = (
        ModuleName::new(module.to_owned()),
        FunctionName::new(function.to_owned()),
    ) else {
        return;
    };
    let mfa = Mfa::new(m, f, Arity::new(arity));
    b.external_calls.push(CallSite {
        caller: caller.clone(),
        callee: CallTarget::External(FunctionRef::Concrete(mfa)),
        kind: CallKind::FunRef,
        loc: span_loc(b, start, end),
    });
}

fn caller_mfa(b: &ModuleBuilder, caller: &FunctionSig) -> Option<Mfa> {
    let module = b.name.clone()?;
    Some(Mfa::new(module, caller.name.clone(), caller.arity))
}
