// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! OTP 28 `-nominal` declares a type with the same grammar as `-type`;
//! the declared surface is identical, so a nominal snapshots as a type.

use backhopper_erlang::ErlangExtractor;

#[test]
fn a_nominal_snapshots_beside_a_type_twin_identically() {
    let src = "-module(ra_units).\n\
               -type meters_t() :: integer().\n\
               -nominal meters() :: integer().\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.types.len(), 2);
    let twin = m
        .types
        .iter()
        .find(|t| t.name.as_str() == "meters_t")
        .unwrap();
    let nominal = m
        .types
        .iter()
        .find(|t| t.name.as_str() == "meters")
        .unwrap();
    assert_eq!(nominal.arity, twin.arity);
    assert_eq!(nominal.rhs, twin.rhs);
    assert!(m.opaques.is_empty());
}

#[test]
fn a_multi_line_nominal_body_with_a_union_across_lines_parses_whole() {
    let src = "-module(ra_units).\n\
               -nominal unit() ::\n\
                   meter |\n\
                   second |\n\
                   kilogram.\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.types.len(), 1);
    assert_eq!(m.types[0].name.as_str(), "unit");
    assert!(m.types[0].rhs.contains("kilogram"));
}

#[test]
fn an_export_type_naming_a_nominal_resolves_against_it() {
    let src = "-module(ra_units).\n\
               -export_type([meters/0]).\n\
               -nominal meters() :: integer().\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.export_types.len(), 1);
    assert_eq!(m.export_types[0].name.as_str(), "meters");
    assert!(
        m.types
            .iter()
            .any(|t| t.name == m.export_types[0].name && t.arity == m.export_types[0].arity)
    );
}
