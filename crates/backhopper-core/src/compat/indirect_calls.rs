// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Extractor for `m:f/a` references carried as arguments to a known
//! indirection form: meck expectations, `rpc:call`, `erpc:call`, and
//! the RabbitMQ CT broker rpc helpers. The atoms naming the referenced
//! function are argument payload to the call-site extractor, so without
//! this pass a test ported from a newer branch can reference a function
//! the target branch never had and the target-tree axis reports
//! nothing. One level of unwrapping covers a meck expectation reached
//! through an rpc helper, the shape the motivating round shipped.
//!
//! A non-literal module or function is skipped without counting: it is
//! not a reference this axis claims to see. A literal `m:f` whose arity
//! source is unreadable is counted as withheld, never guessed.

use std::str::FromStr;

use backhopper_erlang_scan::{ScannedArgs, ScannedList, scan_list_elements, scan_top_level_args};

use crate::compat::call_sites::{body_runs, call_re, run_line_at};
use crate::model::names::{Arity, FunctionName, Mfa, ModuleName};
use crate::model::verdict::IndirectCallForm;

/// One statically readable `m:f/a` carried as arguments to a
/// recognized indirection form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndirectCallSite {
    pub mfa: Mfa,
    pub via: IndirectCallForm,
    pub line: u32,
}

/// Extraction result: the readable sites plus the count of occurrences
/// whose module and function were literal atoms but whose arity source
/// was not readable.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IndirectExtraction {
    pub sites: Vec<IndirectCallSite>,
    pub withheld_dynamic: usize,
}

/// Where a recognized form carries its module, function, and arity.
/// Positions are argument indexes; the arity source names both the
/// index and how to read it.
struct FormSpec {
    via: IndirectCallForm,
    module_pos: usize,
    function_pos: usize,
    arity: AritySource,
}

enum AritySource {
    IntegerLiteral(usize),
    FunExpression(usize),
    ArgumentList(usize),
}

/// The form table, keyed by the called module, function, and argument
/// count. The CT broker helper is overloaded with the module at
/// different positions per arity, which is why the count is part of
/// the key.
fn form_spec(module: &str, function: &str, argc: usize) -> Option<FormSpec> {
    let spec = |via, module_pos, function_pos, arity| FormSpec {
        via,
        module_pos,
        function_pos,
        arity,
    };
    match (module, function, argc) {
        ("meck", "expect", 3) => Some(spec(
            IndirectCallForm::MeckExpect,
            0,
            1,
            AritySource::FunExpression(2),
        )),
        ("meck", "expect", 4) => Some(spec(
            IndirectCallForm::MeckExpect,
            0,
            1,
            AritySource::IntegerLiteral(2),
        )),
        ("rpc", "call", 4 | 5) => Some(spec(
            IndirectCallForm::RpcCall,
            1,
            2,
            AritySource::ArgumentList(3),
        )),
        ("erpc", "call", 4 | 5) => Some(spec(
            IndirectCallForm::ErpcCall,
            1,
            2,
            AritySource::ArgumentList(3),
        )),
        ("rabbit_ct_broker_helpers", "rpc", 4) | ("rabbit_ct_broker_helpers", "rpc_all", 4 | 5) => {
            Some(spec(
                IndirectCallForm::CtBrokerHelperRpc,
                1,
                2,
                AritySource::ArgumentList(3),
            ))
        }
        ("rabbit_ct_broker_helpers", "rpc", 5 | 6) => Some(spec(
            IndirectCallForm::CtBrokerHelperRpc,
            2,
            3,
            AritySource::ArgumentList(4),
        )),
        _ => None,
    }
}

/// Every statically readable indirect MFA reference in `src`, scanned
/// over the same joined body runs as the qualified-call extractor so a
/// wrapped form recovers its exact arity. `line_map[i]` is text line
/// `i`'s file line.
pub fn extract_indirect_calls(src: &str, line_map: &[u32]) -> IndirectExtraction {
    let mut out = IndirectExtraction::default();
    for run in body_runs(src, line_map) {
        for caps in call_re().captures_iter(&run.text) {
            let group = caps.get(0).expect("capture");
            let ScannedArgs::Terminated { args, .. } =
                scan_top_level_args(&run.text[group.end()..])
            else {
                continue;
            };
            let Some(spec) = form_spec(&caps[1], &caps[2], args.len()) else {
                continue;
            };
            let line = run_line_at(&run.line_starts, group.start());
            read_reference(&spec, &args, line, &mut out);
        }
    }
    out
}

/// Reads one occurrence of a recognized form. A non-atom module or
/// function skips silently; a readable pair with an unreadable arity
/// counts as withheld.
fn read_reference(spec: &FormSpec, args: &[&str], line: u32, out: &mut IndirectExtraction) {
    let (Some(module), Some(function)) = (
        args.get(spec.module_pos).and_then(|a| parse_atom(a)),
        args.get(spec.function_pos).and_then(|a| parse_atom(a)),
    ) else {
        return;
    };
    let (Ok(module), Ok(function)) = (
        ModuleName::from_str(&module),
        FunctionName::from_str(&function),
    ) else {
        return;
    };
    let arity = match &spec.arity {
        AritySource::IntegerLiteral(pos) => integer_arity(args.get(*pos)),
        AritySource::FunExpression(pos) => args.get(*pos).and_then(|a| fun_arity(a)),
        AritySource::ArgumentList(pos) => match list_elements(args.get(*pos)) {
            Some(elements) => {
                // One level of unwrapping: the rpc'd callee is itself
                // a meck expectation, so the real reference is inside
                // the argument list.
                if module.as_str() == "meck" && function.as_str() == "expect" {
                    if let Some(inner) = form_spec("meck", "expect", elements.len()) {
                        let elements: Vec<&str> = elements.iter().map(String::as_str).collect();
                        read_reference(&inner, &elements, line, out);
                    } else {
                        out.withheld_dynamic += 1;
                    }
                    return;
                }
                Arity::try_from(elements.len()).ok()
            }
            None => None,
        },
    };
    match arity {
        Some(arity) => out.sites.push(IndirectCallSite {
            mfa: Mfa::new(module, function, arity),
            via: spec.via,
            line,
        }),
        None => out.withheld_dynamic += 1,
    }
}

/// A bare or quoted literal atom, or `None` for anything else.
fn parse_atom(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('\'') {
        let inner = inner.strip_suffix('\'')?;
        if inner.is_empty() || inner.contains('\'') {
            return None;
        }
        return Some(inner.to_owned());
    }
    let mut chars = raw.chars();
    let first = chars.next()?;
    if first.is_ascii_lowercase()
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@')
    {
        Some(raw.to_owned())
    } else {
        None
    }
}

/// An integer-literal arity, or `None`.
fn integer_arity(raw: Option<&&str>) -> Option<Arity> {
    let raw = raw?.trim();
    let value: usize = raw.parse().ok()?;
    Arity::try_from(value).ok()
}

/// Arity of a `fun` expression: a `fun(...) ->` head or a `fun [m:]f/N`
/// capture. `None` for anything else, including a variable.
fn fun_arity(raw: &str) -> Option<Arity> {
    let rest = raw.trim().strip_prefix("fun")?;
    // Only `fun(` or `fun ` opens a fun expression: an atom such as
    // `funny(...)` must not match.
    if !rest.starts_with('(') && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    if let Some(after_paren) = rest.strip_prefix('(') {
        let ScannedArgs::Terminated { args, .. } = scan_top_level_args(after_paren) else {
            return None;
        };
        return Arity::try_from(args.len()).ok();
    }
    // A capture's arity is the digits after the final `/`, whatever
    // the module and function parts are.
    let (_, digits) = rest.rsplit_once('/')?;
    let value: usize = digits.trim().parse().ok()?;
    Arity::try_from(value).ok()
}

/// The elements of a literal list argument, or `None` when the
/// argument is not a plain terminated list.
fn list_elements(raw: Option<&&str>) -> Option<Vec<String>> {
    let raw = raw?.trim();
    let after_bracket = raw.strip_prefix('[')?;
    match scan_list_elements(after_bracket) {
        // Trailing text after the close bracket (`[...] ++ More`)
        // means the argument is not just this list.
        ScannedList::Terminated { elements, consumed }
            if after_bracket[consumed..].trim().is_empty() =>
        {
            Some(elements.into_iter().map(str::to_owned).collect())
        }
        ScannedList::Terminated { .. } | ScannedList::ImproperTail | ScannedList::Unterminated => {
            None
        }
    }
}
