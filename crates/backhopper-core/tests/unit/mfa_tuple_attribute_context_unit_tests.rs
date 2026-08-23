// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_qualified_calls`: which lines the attribute-only run feeds
//! into the tuple scan, as opposed to the shape itself
//! (`mfa_tuple_shape_unit_tests.rs`).

use backhopper_core::compat::call_sites::extract_qualified_calls;

fn seq_map(src: &str) -> Vec<u32> {
    (1..=src.lines().count() as u32).collect()
}

fn calls(src: &str) -> Vec<(String, String, u8, u32)> {
    extract_qualified_calls(src, &seq_map(src))
        .into_iter()
        .map(|c| {
            (
                c.mfa.module.to_string(),
                c.mfa.function.to_string(),
                c.mfa.arity.get(),
                c.line,
            )
        })
        .collect()
}

#[test]
fn the_real_boot_step_form_is_found_at_the_mfa_fields_line() {
    let src = "-rabbit_boot_step({logger_exchange,\n\
                   [{description, \"log exchange\"},\n\
                    {mfa, {rabbit_logger_exchange_h, declare_exchange, []}},\n\
                    {requires, core_initialized}]}).\n";
    let got = calls(src);
    assert_eq!(
        got,
        [(
            "rabbit_logger_exchange_h".to_owned(),
            "declare_exchange".to_owned(),
            0,
            3
        )]
    );
}

#[test]
fn an_attribute_wrapped_across_four_lines_still_joins() {
    let src = "-rabbit_boot_step({logger_exchange,\n\
                   [{mfa,\n\
                     {rabbit_logger_exchange_h,\n\
                      declare_exchange, []}}]}).\n";
    let got = calls(src);
    assert_eq!(
        got,
        [(
            "rabbit_logger_exchange_h".to_owned(),
            "declare_exchange".to_owned(),
            0,
            3
        )]
    );
}

#[test]
fn a_cleanup_field_beside_mfa_yields_a_second_reference() {
    let src = "-rabbit_boot_step({logger_exchange,\n\
                   [{mfa, {rabbit_logger_exchange_h, declare_exchange, []}},\n\
                    {cleanup, {rabbit_logger_exchange_h, cleanup_exchange, []}}]}).\n";
    let mut got = calls(src);
    got.sort();
    assert_eq!(
        got,
        [
            (
                "rabbit_logger_exchange_h".to_owned(),
                "cleanup_exchange".to_owned(),
                0,
                3
            ),
            (
                "rabbit_logger_exchange_h".to_owned(),
                "declare_exchange".to_owned(),
                0,
                2
            ),
        ]
    );
}

#[test]
fn a_body_line_directly_above_the_attribute_is_excluded() {
    let src = "f() -> {m2, f2, []}.\n\
               -rabbit_boot_step({mfa, {rabbit_logger_exchange_h, declare_exchange, []}}).\n";
    let got = calls(src);
    assert_eq!(
        got,
        [(
            "rabbit_logger_exchange_h".to_owned(),
            "declare_exchange".to_owned(),
            0,
            2
        )]
    );
}

#[test]
fn the_same_tuple_in_body_context_is_not_scanned() {
    let src = "f() -> {rabbit_logger_exchange_h, declare_exchange, []}.\n";
    let got = calls(src);
    assert!(got.is_empty(), "unexpected: {got:?}");
}

#[test]
fn a_record_field_default_of_the_shape_is_not_scanned() {
    let src =
        "-record(state, {handler = {rabbit_logger_exchange_h, declare_exchange, []} :: term()}).\n";
    let got = calls(src);
    assert!(got.is_empty(), "unexpected: {got:?}");
}
