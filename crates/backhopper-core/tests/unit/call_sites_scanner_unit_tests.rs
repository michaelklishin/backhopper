// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The unified argument scanner: arity through nested terms, binaries,
//! char literals, and comments; `FunctionAnyArity` for wrapped
//! calls; `fun M:F/A` remote fun references.

use std::str::FromStr;

use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::{
    DynamicCall, ScannedArgs, extract_call_args_into, extract_definitions_into,
    extract_dynamic_into, extract_into, extract_into_with_macros, scan_top_level_args,
    strip_line_comment,
};
use backhopper_core::erlang_macros::{MacroKey, MacroTable};
use backhopper_core::model::names::{FunctionName, ModuleName};
use backhopper_core::model::symbol::SymbolRef;

fn function_names(out: &[SymbolRef]) -> Vec<String> {
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

fn any_arity_names(out: &[SymbolRef]) -> Vec<String> {
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::FunctionAnyArity { module, function } => {
                Some(format!("{module}:{function}"))
            }
            _ => None,
        })
        .collect()
}

// Verified false positive: commas in the nested tuple counted, reporting match_qs/4 for a /2 call.
#[test]
fn arity_counts_through_nested_tuples_and_lists() {
    let mut out = Vec::new();
    extract_into(
        "cowboy_req:match_qs([{download, [Constraint], undefined}], ReqData)",
        &mut out,
    );
    assert!(function_names(&out).contains(&"cowboy_req:match_qs/2".to_owned()));
}

#[test]
fn arity_counts_through_binaries() {
    let mut out = Vec::new();
    extract_into("m:f(<<1,2,3>>, X)", &mut out);
    assert!(function_names(&out).contains(&"m:f/2".to_owned()));
}

#[test]
fn arity_counts_through_char_literal_comma() {
    let mut out = Vec::new();
    extract_into("m:f($,, X)", &mut out);
    assert!(function_names(&out).contains(&"m:f/2".to_owned()));
}

#[test]
fn char_literal_quote_does_not_corrupt_string_state() {
    let mut out = Vec::new();
    extract_into("m:f($\", X), other:call(Y)", &mut out);
    let names = function_names(&out);
    assert!(names.contains(&"m:f/2".to_owned()));
    assert!(names.contains(&"other:call/1".to_owned()));
}

// A wrapped call has no trustworthy arity on this line: report any-arity, never a guessed exact one.
#[test]
fn wrapped_call_yields_any_arity_reference() {
    let mut out = Vec::new();
    extract_into("m:f(A,", &mut out);
    assert!(function_names(&out).is_empty());
    assert!(any_arity_names(&out).contains(&"m:f".to_owned()));
}

#[test]
fn commented_out_call_does_not_extract() {
    let mut out = Vec::new();
    extract_into("    X = 1, % m:f(A, B) used to live here", &mut out);
    assert!(function_names(&out).is_empty());
    assert!(any_arity_names(&out).is_empty());
}

#[test]
fn comment_after_call_does_not_hide_it() {
    let mut out = Vec::new();
    extract_into("m:f(A) % see m:g/2", &mut out);
    assert!(function_names(&out).contains(&"m:f/1".to_owned()));
    assert_eq!(function_names(&out).len(), 1);
}

#[test]
fn percent_inside_string_is_not_a_comment() {
    assert_eq!(
        strip_line_comment(r#"m:f("100%", X) % real comment"#),
        r#"m:f("100%", X) "#
    );
}

#[test]
fn percent_as_char_literal_is_not_a_comment() {
    assert_eq!(strip_line_comment("m:f($%, X)"), "m:f($%, X)");
}

#[test]
fn fun_ref_extracts_with_exact_arity() {
    let mut out = Vec::new();
    extract_into(
        "Constraint = cowboy_constraints:from_fun(fun cow_http:ensure_token/1)",
        &mut out,
    );
    let names = function_names(&out);
    assert!(names.contains(&"cowboy_constraints:from_fun/1".to_owned()));
    assert!(names.contains(&"cow_http:ensure_token/1".to_owned()));
}

#[test]
fn fun_ref_tolerates_whitespace() {
    let mut out = Vec::new();
    extract_into("F = fun m : f / 3", &mut out);
    assert!(function_names(&out).contains(&"m:f/3".to_owned()));
}

#[test]
fn fun_ref_with_macro_module_resolves_through_the_table() {
    let mut table = MacroTable::new();
    table.insert(
        MacroKey {
            name: "THE_MOD".to_owned(),
            arity: None,
        },
        "dep_mod".to_owned(),
    );
    let mut out = Vec::new();
    extract_into_with_macros("F = fun ?THE_MOD:go/2", &table, &mut out);
    assert!(function_names(&out).contains(&"dep_mod:go/2".to_owned()));
}

#[test]
fn fun_ref_with_variable_slots_classifies_as_dynamic() {
    for line in ["fun Mod:f/1", "fun m:F/1", "fun m:f/Arity"] {
        let mut refs = Vec::new();
        extract_into(line, &mut refs);
        assert!(
            function_names(&refs).is_empty(),
            "{line} must not extract a literal ref"
        );
        let mut dynamic = Vec::new();
        extract_dynamic_into(line, &mut dynamic);
        assert!(
            dynamic.contains(&DynamicCall::VariableDispatch),
            "{line} must classify as dynamic"
        );
    }
}

#[test]
fn local_fun_ref_is_not_extracted() {
    let mut out = Vec::new();
    extract_into("F = fun local_helper/2", &mut out);
    assert!(function_names(&out).is_empty());
    assert!(any_arity_names(&out).is_empty());
}

#[test]
fn scan_reports_unterminated_for_wrapped_args() {
    assert!(matches!(
        scan_top_level_args("A, B"),
        ScannedArgs::Unterminated { .. }
    ));
    match scan_top_level_args("A, B)") {
        ScannedArgs::Terminated { args, consumed } => {
            assert_eq!(args, vec!["A", " B"]);
            assert_eq!(consumed, 5);
        }
        other @ ScannedArgs::Unterminated { .. } => panic!("expected terminated, got {other:?}"),
    }
}

#[test]
fn wrapped_call_args_do_not_feed_clause_mismatch() {
    let mut out = Vec::new();
    extract_call_args_into("m:f(A,", &mut out);
    assert!(out.is_empty());
}

#[test]
fn wrapped_definition_head_lands_as_any_arity() {
    let mut out = Vec::new();
    extract_definitions_into("handle_call(Request,", &mut out);
    assert_eq!(any_arity_names(&out), vec!["_local:handle_call".to_owned()]);
}

#[test]
fn any_arity_symbol_parses_back_from_str() {
    // The m:f/? rendering is display-only; the symbol carries module and function newtypes.
    let m = ModuleName::from_str("m").unwrap();
    let f = FunctionName::from_str("f").unwrap();
    let s = SymbolRef::function_any_arity(m, f);
    assert!(matches!(s.kind, SymbolKind::FunctionAnyArity { .. }));
}
