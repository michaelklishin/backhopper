// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Generated `-record(...)` field lines, checked against the real
//! masking path in `extract_qualified_calls_with_context` and
//! `scan_hunk`: a field's `::` annotation must never surface as a
//! call, its default expression's call must survive, and the
//! annotation's own type references must still be recoverable.

use proptest::prelude::*;

use backhopper_core::compat::added_lines::added_lines_with_context;
use backhopper_core::compat::call_sites::{extract_qualified_calls_with_context, scan_hunk};
use backhopper_core::compat::patch::{Hunk, HunkLine};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::symbol::{RefOrigin, SymbolKind};

fn ident() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}"
}

fn arg_list(n: usize) -> String {
    (0..n)
        .map(|i| format!("a{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `(module, name)` pairs for a default call and an annotation type,
/// distinct enough that checking one's absence never collides with
/// the other's presence.
fn distinct_pair() -> impl Strategy<Value = ((String, String), (String, String))> {
    ((ident(), ident(), 0usize..3), (ident(), ident(), 0usize..3)).prop_filter_map(
        "modules or names collide",
        |((dm, df, da), (am, at, aa))| {
            if dm == am && df == at {
                return None;
            }
            Some((
                (dm, format!("{df}({})", arg_list(da))),
                (am, format!("{at}({})", arg_list(aa))),
            ))
        },
    )
}

fn record_field_line(default: &(String, String), annotation: &(String, String)) -> String {
    let (dm, dcall) = default;
    let (am, acall) = annotation;
    format!("    x = {dm}:{dcall} :: {am}:{acall},")
}

fn scan_qualified_calls(line: &str) -> Vec<String> {
    let hunk = Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 2,
        lines: vec![
            HunkLine::Context("-record(r, {".into()),
            HunkLine::Added(line.to_owned()),
        ],
    };
    let (added, line_map, ctx) = added_lines_with_context(std::slice::from_ref(&hunk));
    extract_qualified_calls_with_context(&added, &line_map, &ctx)
        .into_iter()
        .map(|c| c.mfa.to_string())
        .collect()
}

fn scan_types_and_functions(line: &str) -> (Vec<String>, Vec<String>) {
    let out = scan_hunk(
        &[
            (RefOrigin::Added, "-record(r, {"),
            (RefOrigin::Added, line),
            (RefOrigin::Added, "})."),
        ],
        &MacroTable::new(),
    );
    let types: Vec<String> = out
        .referenced
        .iter()
        .filter_map(|r| match &r.kind {
            SymbolKind::Type {
                module,
                name,
                arity,
            } => Some(format!("{module}:{name}/{arity}")),
            _ => None,
        })
        .collect();
    let functions: Vec<String> = out
        .referenced
        .iter()
        .filter_map(|r| match &r.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    (types, functions)
}

proptest! {
    #[test]
    fn no_qualified_call_lands_inside_a_type_span(
        (default, annotation) in distinct_pair(),
    ) {
        let line = record_field_line(&default, &annotation);
        let calls = scan_qualified_calls(&line);
        let (am, acall) = &annotation;
        let annotation_name = acall.split('(').next().unwrap();
        prop_assert!(
            !calls.iter().any(|c| c.starts_with(&format!("{am}:{annotation_name}/"))),
            "annotation leaked as a call: {calls:?}"
        );
    }

    #[test]
    fn a_default_call_survives_the_mask(
        (default, annotation) in distinct_pair(),
    ) {
        let line = record_field_line(&default, &annotation);
        let calls = scan_qualified_calls(&line);
        let (dm, dcall) = &default;
        let default_name = dcall.split('(').next().unwrap();
        prop_assert!(
            calls.iter().any(|c| c.starts_with(&format!("{dm}:{default_name}/"))),
            "default call went missing: {calls:?}, line: {line:?}"
        );
    }

    #[test]
    fn every_generated_annotation_type_ref_is_extracted(
        (default, annotation) in distinct_pair(),
    ) {
        let line = record_field_line(&default, &annotation);
        let (types, functions) = scan_types_and_functions(&line);
        let (am, acall) = &annotation;
        let annotation_name = acall.split('(').next().unwrap();
        prop_assert!(
            types.iter().any(|t| t.starts_with(&format!("{am}:{annotation_name}/"))),
            "annotation type ref not recovered: {types:?}, line: {line:?}"
        );
        prop_assert!(
            !functions.iter().any(|f| f.starts_with(&format!("{am}:{annotation_name}/"))),
            "annotation leaked into functions: {functions:?}"
        );
        let (dm, dcall) = &default;
        let default_name = dcall.split('(').next().unwrap();
        prop_assert!(
            !types.iter().any(|t| t.starts_with(&format!("{dm}:{default_name}/"))),
            "default leaked into types: {types:?}"
        );
    }
}
