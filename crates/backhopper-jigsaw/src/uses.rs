// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::ops::Range;
use std::str::FromStr;
use std::sync::LazyLock;

use backhopper_core::compat::call_sites::{
    extract_type_refs_into, line_context, strip_line_comment,
};
use backhopper_core::model::names::{
    Arity, BehaviourName, FieldName, FunctionName, MacroName, Mfa, ModuleName, RecordName, TypeName,
};
use backhopper_core::model::symbol::{RefContext, SymbolKind, SymbolRef};
use backhopper_erlang_scan::{
    ScanArity, hash_inside_number, matching_bracket, scan_arity, split_top_level_commas,
};
use regex::Regex;

use crate::source::Code;

static DEFINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*-[ \t]*define[ \t]*\(").expect("regex"));
static RECORD_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*-[ \t]*record[ \t]*\([ \t]*([a-z][A-Za-z0-9_@]*)[ \t]*,")
        .expect("regex")
});
static MACRO_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?[ \t]*([A-Za-z_][A-Za-z0-9_]*)").expect("regex"));
static RECORD_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#[ \t]*([a-z][A-Za-z0-9_@]*)[ \t]*(\{|\.)").expect("regex"));
static BEHAVIOUR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*-[ \t]*behaviou?r[ \t]*\([ \t]*([a-z][A-Za-z0-9_@]*)[ \t]*\)")
        .expect("regex")
});
static ELIXIR_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"'(Elixir\.[A-Za-z0-9_.]+)'[ \t]*:[ \t]*([a-z][A-Za-z0-9_@]*)[ \t]*\(")
        .expect("regex")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MacroUse {
    pub name: MacroName,
    pub arity: Option<Arity>,
    pub line: u32,
}

/// A `#name{...}` or `#name.field` in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordUse {
    pub name: RecordName,
    pub line: u32,
    pub construction: Option<Construction>,
}

/// What a record expression that builds a new record sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Construction {
    pub set: BTreeSet<FieldName>,
    pub sets_the_rest: bool,
}

/// The macros a file defines, by name: a macro the plugin set defines
/// shadows a host macro of the same name whatever its arity.
pub(crate) fn defined_macros(code: &Code) -> BTreeSet<MacroName> {
    define_forms(code)
        .into_iter()
        .map(|form| form.name)
        .collect()
}

pub(crate) fn declared_records(code: &Code) -> BTreeSet<RecordName> {
    RECORD_DECL
        .captures_iter(code.bare())
        .filter_map(|caps| RecordName::from_str(&caps[1]).ok())
        .collect()
}

struct DefineForm {
    name: MacroName,
    params: BTreeSet<String>,
    span: Range<usize>,
}

fn define_forms(code: &Code) -> Vec<DefineForm> {
    let text = code.bare();
    let bytes = text.as_bytes();
    DEFINE
        .find_iter(text)
        .filter_map(|found| {
            let open = found.end() - 1;
            let close = matching_bracket(bytes, open)?;
            let inner = &text[open + 1..close];
            let head = split_top_level_commas(inner).first().copied()?;
            let head = head.trim();
            let (name, params) = match head.split_once('(') {
                Some((name, rest)) => (
                    name.trim(),
                    rest.trim_end()
                        .trim_end_matches(')')
                        .split(',')
                        .map(str::trim)
                        .map(str::to_owned)
                        .collect(),
                ),
                None => (head, BTreeSet::new()),
            };
            Some(DefineForm {
                name: MacroName::from_str(name).ok()?,
                params,
                span: open..close,
            })
        })
        .collect()
}

/// Every `?NAME` and `?NAME(...)`. A `-define` parameter used in its own
/// body is not a macro use.
pub(crate) fn macro_uses(code: &Code) -> Vec<MacroUse> {
    let text = code.bare();
    let forms = define_forms(code);
    let mut out = Vec::new();
    for caps in MACRO_USE.captures_iter(text) {
        let whole = caps.get(0).expect("capture");
        if text[..whole.start()].ends_with('?') {
            continue;
        }
        let name = &caps[1];
        let in_own_params = forms
            .iter()
            .any(|form| form.span.contains(&whole.start()) && form.params.contains(name));
        if in_own_params {
            continue;
        }
        let Ok(name) = MacroName::from_str(name) else {
            continue;
        };
        let rest = text[whole.end()..].trim_start();
        let arity = rest
            .strip_prefix('(')
            .and_then(|after| match scan_arity(after) {
                ScanArity::Exact(n) => Some(Arity::new(n)),
                ScanArity::Unterminated => None,
            });
        out.push(MacroUse {
            name,
            arity,
            line: code.line_at(whole.start()),
        });
    }
    out
}

/// Byte ranges of function clause heads: from a column-one lowercase
/// start to the first `->`.
fn clause_heads(text: &str) -> Vec<Range<usize>> {
    let mut heads = Vec::new();
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let starts_clause = line
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '\'');
        if starts_clause && let Some(arrow) = text[offset..].find("->") {
            heads.push(offset..offset + arrow);
        }
        offset += line.len();
    }
    heads
}

fn follows_expression(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ')' | ']' | '}' | '>'))
}

/// Whether what follows a record's closing brace makes it a pattern: a
/// match, a clause arrow, a guard or a generator.
fn pattern_follows(after: &str) -> bool {
    let rest = after.trim_start();
    if rest.starts_with("->") || rest.starts_with("<-") || rest.starts_with("when") {
        return true;
    }
    rest.strip_prefix('=').is_some_and(|tail| {
        !(tail.starts_with('=')
            || tail.starts_with(':')
            || tail.starts_with('/')
            || tail.starts_with('<'))
    })
}

fn construction_of(inner: &str) -> Construction {
    let mut set = BTreeSet::new();
    let mut sets_the_rest = false;
    for item in split_top_level_commas(inner) {
        let Some((field, _)) = item.split_once('=') else {
            continue;
        };
        let field = field.trim();
        if field == "_" {
            sets_the_rest = true;
        } else if let Ok(name) = FieldName::from_str(field) {
            set.insert(name);
        }
    }
    Construction { set, sets_the_rest }
}

/// Every `#name{...}` and `#name.field`. In a function body, a `#name{...}`
/// that is not an update and not a pattern is a construction. A pattern
/// that none of the heuristics recognise reads as a construction, which can
/// only add a warning.
pub(crate) fn record_uses(code: &Code, source: &str) -> Vec<RecordUse> {
    let text = code.bare();
    let bytes = text.as_bytes();
    let heads = clause_heads(text);
    let contexts = line_context(source);
    let mut out = Vec::new();
    for caps in RECORD_USE.captures_iter(text) {
        let whole = caps.get(0).expect("capture");
        if hash_inside_number(bytes, whole.start()) {
            continue;
        }
        let Ok(name) = RecordName::from_str(&caps[1]) else {
            continue;
        };
        let line = code.line_at(whole.start());
        let in_body = contexts
            .get(line as usize - 1)
            .is_some_and(|class| class.context == RefContext::Body);
        let construction = if &caps[2] == "{"
            && in_body
            && !follows_expression(text, whole.start())
            && !heads.iter().any(|head| head.contains(&whole.start()))
        {
            let open = whole.end() - 1;
            matching_bracket(bytes, open)
                .filter(|&close| !pattern_follows(&text[close + 1..]))
                .map(|close| construction_of(&text[open + 1..close]))
        } else {
            None
        };
        out.push(RecordUse {
            name,
            line,
            construction,
        });
    }
    out
}

/// The behaviours a file declares with their lines.
pub(crate) fn behaviours(code: &Code) -> Vec<(BehaviourName, u32)> {
    BEHAVIOUR
        .captures_iter(code.bare())
        .filter_map(|caps| {
            let whole = caps.get(0).expect("capture");
            Some((
                BehaviourName::from_str(&caps[1]).ok()?,
                code.line_at(whole.start()),
            ))
        })
        .collect()
}

/// Remote type references in `-spec`, `-callback`, `-type`, `-opaque` and
/// record field annotations, with their lines.
pub(crate) fn type_refs(source: &str) -> Vec<(ModuleName, TypeName, Arity, u32)> {
    let mut out = Vec::new();
    for (index, (line, class)) in source.lines().zip(line_context(source)).enumerate() {
        let line_no = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let mut refs: Vec<SymbolRef> = Vec::new();
        match class.context {
            RefContext::TypeAttribute => extract_type_refs_into(line, &mut refs),
            RefContext::RecordAttribute => {
                let stripped = strip_line_comment(line);
                for span in &class.type_spans {
                    if let Some(part) = stripped.get(span.clone()) {
                        extract_type_refs_into(part, &mut refs);
                    }
                }
            }
            RefContext::Body | RefContext::OtherAttribute | RefContext::AttributeString => {}
        }
        for symbol in refs {
            if let SymbolKind::Type {
                module,
                name,
                arity,
            } = symbol.kind
            {
                out.push((module, name, arity, line_no));
            }
        }
    }
    out
}

/// `'Elixir.Module':function(...)` calls, which the call reader skips.
pub(crate) fn elixir_calls(code: &Code) -> Vec<(Mfa, u32)> {
    let text = code.masked();
    let mut out = Vec::new();
    for caps in ELIXIR_CALL.captures_iter(text) {
        let whole = caps.get(0).expect("capture");
        let ScanArity::Exact(arity) = scan_arity(&text[whole.end()..]) else {
            continue;
        };
        let (Ok(module), Ok(function)) = (
            ModuleName::from_str(&caps[1]),
            FunctionName::from_str(&caps[2]),
        ) else {
            continue;
        };
        out.push((
            Mfa::new(module, function, Arity::new(arity)),
            code.line_at(whole.start()),
        ));
    }
    out
}
