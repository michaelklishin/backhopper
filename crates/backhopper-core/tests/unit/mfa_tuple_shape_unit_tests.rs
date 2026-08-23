// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_mfa_tuples_with_macros`: the `{M, F, Args}` tuple shape,
//! independent of attribute-context wiring
//! (`mfa_tuple_attribute_context_unit_tests.rs`).

use backhopper_core::compat::call_sites::extract_mfa_tuples_with_macros;
use backhopper_core::erlang_macros::{MacroKey, MacroTable};

fn mfas(source: &str) -> Vec<String> {
    extract_mfa_tuples_with_macros(source, &MacroTable::new())
        .iter()
        .map(|mfa| mfa.to_string())
        .collect()
}

fn mfas_with_macros(source: &str, macros: &MacroTable) -> Vec<String> {
    extract_mfa_tuples_with_macros(source, macros)
        .iter()
        .map(|mfa| mfa.to_string())
        .collect()
}

#[test]
fn a_top_level_mfa_tuple_resolves() {
    let out = mfas("{worker, init, [cfg]}");
    assert_eq!(out, ["worker:init/1"]);
}

#[test]
fn the_real_boot_step_form_finds_the_nested_tuple() {
    let src = "-rabbit_boot_step({logger_exchange,\n\
                   [{description, \"log exchange\"},\n\
                    {mfa, {rabbit_logger_exchange_h, declare_exchange, []}},\n\
                    {requires, core_initialized}]}).\n";
    let out = mfas(src);
    assert_eq!(out, ["rabbit_logger_exchange_h:declare_exchange/0"]);
}

#[test]
fn a_cleanup_field_beside_mfa_yields_a_second_reference() {
    let src = "{logger_exchange,\n\
                [{mfa, {rabbit_logger_exchange_h, declare_exchange, []}},\n\
                 {cleanup, {rabbit_logger_exchange_h, cleanup_exchange, []}}]}";
    let mut out = mfas(src);
    out.sort();
    assert_eq!(
        out,
        [
            "rabbit_logger_exchange_h:cleanup_exchange/0",
            "rabbit_logger_exchange_h:declare_exchange/0",
        ]
    );
}

#[test]
fn quoted_atom_module_and_function_resolve() {
    let out = mfas("{'rabbit_logger_exchange_h', 'declare_exchange', []}");
    assert_eq!(out, ["rabbit_logger_exchange_h:declare_exchange/0"]);
}

#[test]
fn a_macro_module_head_resolves_through_the_table() {
    let mut macros = MacroTable::new();
    macros.insert(
        MacroKey {
            name: "HANDLER".into(),
            arity: None,
        },
        "rabbit_logger_exchange_h".into(),
    );
    let out = mfas_with_macros("{?HANDLER, declare_exchange, []}", &macros);
    assert_eq!(out, ["rabbit_logger_exchange_h:declare_exchange/0"]);
}

#[test]
fn an_unexpandable_macro_yields_nothing() {
    let out = mfas("{?UNBOUND, declare_exchange, []}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_variable_argument_list_is_not_a_literal_list() {
    let out = mfas("{worker, init, Args}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn an_integer_arity_third_element_does_not_match() {
    let out = mfas("{worker, init, 3}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_two_tuple_does_not_match() {
    let out = mfas("{worker, init}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_four_tuple_does_not_match() {
    let out = mfas("{worker, init, [], extra}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_variable_module_does_not_match() {
    let out = mfas("{Mod, init, []}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_variable_function_does_not_match() {
    let out = mfas("{worker, F, []}");
    assert!(out.is_empty(), "unexpected: {out:?}");
}

#[test]
fn a_list_with_nested_tuples_and_lists_counts_top_level_items_only() {
    let out = mfas("{worker, init, [{a, [1, 2]}, b]}");
    assert_eq!(out, ["worker:init/2"]);
}
