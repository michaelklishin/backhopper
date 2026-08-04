// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-attribute(...)` parsers used by the `SourceReader` orchestrator.

use std::path::PathBuf;

use backhopper_core::{Arity, BehaviourName, FunctionName, ModuleName};
use backhopper_erlang_scan::{skip_char_literal_span, split_top_level_commas};
use backhopper_xref_graph::{Deprecation, DeprecationTier, FunctionSig};

use crate::errors::ReadWarning;
use crate::macros::parse_define;
use crate::reader::scan::ModuleBuilder;
use crate::scanner::Scanner;

pub(super) fn consume_attribute_body<'a>(sc: &mut Scanner<'a>) -> &'a str {
    let start = sc.pos().byte_offset as usize;
    let mut depth = 0i32;
    while let Some(byte) = sc.peek() {
        match byte {
            b'(' | b'[' | b'{' => {
                depth += 1;
                sc.advance();
            }
            b')' | b']' | b'}' => {
                depth -= 1;
                sc.advance();
            }
            b'.' if depth <= 0 => {
                let end = sc.pos().byte_offset as usize;
                sc.advance();
                return sc.src_slice(start, end);
            }
            b'"' => sc.consume_string(),
            b'~' => sc.consume_sigil_or_advance(),
            b'\'' => sc.consume_quoted_atom(),
            b'$' => sc.consume_char_literal(),
            b'%' => sc.skip_line(),
            _ => {
                sc.advance();
            }
        }
    }
    sc.src_slice(start, sc.pos().byte_offset as usize)
}

pub(super) fn handle_attribute(
    name: &str,
    body: &str,
    b: &mut ModuleBuilder,
    warnings: &mut Vec<ReadWarning>,
) {
    let trimmed = body
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    match name {
        "module" => {
            if let Ok(m) = ModuleName::new(trimmed.to_owned()) {
                b.set_module(m, warnings);
            } else {
                warnings.push(ReadWarning::UnsupportedQuotedModuleName {
                    path: b.path.clone(),
                    raw: trimmed.to_owned(),
                });
            }
        }
        "export" => {
            for sig in parse_export_list(trimmed) {
                b.explicit_export_decls.insert(sig);
            }
        }
        "behaviour" | "behavior" => {
            if let Ok(beh) = BehaviourName::new(trimmed.to_owned()) {
                b.behaviours.insert(beh);
            }
        }
        "callback" => {
            if let Some(sig) = parse_callback_signature(trimmed) {
                b.callbacks.insert(sig);
            }
        }
        "optional_callbacks" => {
            for sig in parse_export_list(trimmed) {
                b.optional_callbacks.insert(sig);
            }
        }
        "import" => {
            if let Some((module, sigs)) = parse_import(trimmed) {
                b.imports.entry(module).or_default().extend(sigs);
            }
        }
        "deprecated" => {
            for (sig, dep) in parse_deprecated(trimmed) {
                b.deprecated.insert(sig, dep);
            }
        }
        "on_load" => {
            if let Some(sig) = parse_on_load(trimmed) {
                b.on_load = Some(sig);
            }
        }
        "define" => {
            if let Some((key, value)) = parse_define(trimmed) {
                b.macros.insert(key, value);
            }
        }
        "include" | "include_lib" => {
            if let Some(path) = parse_include_path(trimmed) {
                b.include_paths.push(path);
            }
        }
        _ => {}
    }
}

fn parse_include_path(body: &str) -> Option<PathBuf> {
    let s = body
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let s = s.strip_prefix('"')?;
    let s = s.strip_suffix('"')?;
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

fn parse_export_list(body: &str) -> Vec<FunctionSig> {
    let s = body
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    let mut out = Vec::new();
    for item in split_top_level_commas(s) {
        let item = item.trim();
        if let Some((name, arity)) = item.rsplit_once('/') {
            if let (Ok(name), Ok(arity)) = (
                FunctionName::new(name.trim().to_owned()),
                arity.trim().parse::<u8>(),
            ) {
                out.push(FunctionSig::new(name, Arity::new(arity)));
            }
        }
    }
    out
}

fn parse_callback_signature(body: &str) -> Option<FunctionSig> {
    let body = body.trim();
    let (name_raw, after_open) = body.split_once('(')?;
    let name = FunctionName::new(name_raw.trim().to_owned()).ok()?;
    let args = extract_balanced_arg_list(after_open)?;
    let arity = count_top_level_args(args);
    Some(FunctionSig::new(name, Arity::new(arity)))
}

/// Returns the slice between an already-consumed opening `(` and its
/// matching `)`, tracking nested parens and skipping parens inside
/// strings and quoted atoms. Needed for callback signatures like
/// `-callback foo(fun() -> ok) -> ok` where a naive search for the
/// first `->` or first `)` misparses fun-type arguments.
fn extract_balanced_arg_list(after_open: &str) -> Option<&str> {
    let bytes = after_open.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut in_quote = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if !in_string && !in_quote && c == b'$' && i + 1 < bytes.len() {
            i += skip_char_literal_span(bytes, i);
            continue;
        }
        match c {
            b'(' if !in_string && !in_quote => depth += 1,
            b')' if !in_string && !in_quote => {
                if depth == 0 {
                    return Some(&after_open[..i]);
                }
                depth -= 1;
            }
            b'"' if !in_quote => in_string = !in_string,
            b'\'' if !in_string => in_quote = !in_quote,
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_import(body: &str) -> Option<(ModuleName, Vec<FunctionSig>)> {
    let s = body.trim();
    let (module_part, rest) = s.split_once(',')?;
    let module = ModuleName::new(module_part.trim().to_owned()).ok()?;
    Some((module, parse_export_list(rest)))
}

fn parse_deprecated(body: &str) -> Vec<(FunctionSig, Deprecation)> {
    let inner = body
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    let mut out = Vec::new();
    for tup in split_top_level_commas(inner) {
        let tup = tup
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim();
        let parts: Vec<&str> = split_top_level_commas(tup)
            .into_iter()
            .map(str::trim)
            .collect();
        if parts.len() < 2 {
            continue;
        }
        let (Ok(name), Ok(arity)) = (
            FunctionName::new(parts[0].to_owned()),
            parts[1].parse::<u8>(),
        ) else {
            continue;
        };
        let sig = FunctionSig::new(name, Arity::new(arity));
        let mut tier = DeprecationTier::Eventual;
        let mut message: Option<String> = None;
        if let Some(third) = parts.get(2) {
            let t = third.trim().trim_start_matches('"').trim_end_matches('"');
            match t {
                "next_version" | "next_release" | "next_major_release" => {
                    tier = DeprecationTier::Next;
                }
                "now" => tier = DeprecationTier::Now,
                "eventually" => tier = DeprecationTier::Eventual,
                _ => message = Some(t.to_owned()),
            }
        }
        out.push((sig, Deprecation { tier, message }));
    }
    out
}

fn parse_on_load(body: &str) -> Option<FunctionSig> {
    let s = body
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim();
    let (name, arity) = s.split_once(',')?;
    let name = FunctionName::new(name.trim().to_owned()).ok()?;
    let arity = arity.trim().parse::<u8>().ok()?;
    Some(FunctionSig::new(name, Arity::new(arity)))
}

pub(super) fn count_top_level_args(s: &str) -> u8 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let parts = split_top_level_commas(s);
    u8::try_from(parts.len()).unwrap_or(u8::MAX)
}
