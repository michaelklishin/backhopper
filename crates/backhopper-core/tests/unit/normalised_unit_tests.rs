// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
use backhopper_core::model::names::{Arity, MacroName};
use backhopper_core::model::normalised::{NormalisedDefault, NormalisedMacroBody};

fn body(source: &str) -> String {
    NormalisedMacroBody::from_source(source).to_string()
}

#[test]
fn whitespace_runs_collapse_and_ends_are_trimmed() {
    assert_eq!(body("  #{max  =>\n      100 }  "), "#{max => 100 }");
}

#[test]
fn comments_are_stripped() {
    assert_eq!(body("1800000 %% thirty minutes\n"), "1800000");
    assert_eq!(body("{a, % first\n b}"), "{a, b}");
}

#[test]
fn a_percent_inside_a_string_is_kept() {
    assert_eq!(body("\"100% done\""), "\"100% done\"");
}

#[test]
fn a_percent_inside_a_quoted_atom_is_kept() {
    assert_eq!(body("'queue%name'"), "'queue%name'");
}

#[test]
fn a_char_literal_of_a_percent_is_kept() {
    assert_eq!(body("$%"), "$%");
}

#[test]
fn an_empty_body_is_a_token_no_erlang_source_holds() {
    assert_eq!(body(""), "%empty%");
    assert_eq!(body("  %% nothing here\n"), "%empty%");
}

#[test]
fn a_stored_body_must_be_normalised() {
    assert!(NormalisedMacroBody::from_stored("{a, b}").is_ok());
    assert!(NormalisedMacroBody::from_stored("{a,  b}").is_err());
    assert!(NormalisedMacroBody::from_stored(" {a, b}").is_err());
    assert!(NormalisedMacroBody::from_stored("").is_err());
    assert!(NormalisedMacroBody::from_stored("%empty%").is_ok());
}

#[test]
fn normalising_twice_changes_nothing() {
    let once = body("#{a => \"x y\",\n   b => 'c d'} % tail");
    assert_eq!(body(&once), once);
}

#[test]
fn a_default_is_normalised_the_same_way() {
    let default = NormalisedDefault::from_source("[\n  {ssl, [  verify_peer ]}  ]").unwrap();
    assert_eq!(default.as_str(), "[ {ssl, [ verify_peer ]} ]");
}

#[test]
fn bodies_serialize_as_their_text() {
    let json = serde_json::to_string(&NormalisedMacroBody::from_source("1800000")).unwrap();
    assert_eq!(json, "\"1800000\"");
    assert!(serde_json::from_str::<NormalisedMacroBody>("\"1  8\"").is_err());
}

#[test]
fn parse_define_reads_an_object_like_macro() {
    let (key, value) = MacroKey::parse_define("DEFAULT_CONSUMER_TIMEOUT, 1800000").unwrap();
    assert_eq!(key.name.as_str(), "DEFAULT_CONSUMER_TIMEOUT");
    assert_eq!(key.arity, None);
    assert_eq!(value.trim(), "1800000");
}

#[test]
fn parse_define_counts_parameters() {
    let (one, _) = MacroKey::parse_define("IS_QUEUE(Q), is_record(Q, amqqueue)").unwrap();
    assert_eq!(one.arity, Some(Arity::new(1)));
    let (two, _) = MacroKey::parse_define("PAIR(A, B), {A, B}").unwrap();
    assert_eq!(two.arity, Some(Arity::new(2)));
    let (zero, _) = MacroKey::parse_define("NOW(), erlang:monotonic_time()").unwrap();
    assert_eq!(zero.arity, Some(Arity::new(0)));
}

#[test]
fn parse_define_refuses_a_name_that_is_not_a_macro_name() {
    assert!(MacroKey::parse_define("NODE@HOST, 1").is_none());
    assert!(MacroKey::parse_define(", 1").is_none());
    assert!(MacroKey::parse_define("FLAG").is_none());
}

#[test]
fn keys_render_with_the_arity_as_a_suffix_and_parse_back() {
    let key = MacroKey::function_like(MacroName::new("IS_QUEUE").unwrap(), Arity::new(1));
    assert_eq!(key.to_string(), "IS_QUEUE/1");
    assert_eq!("IS_QUEUE/1".parse::<MacroKey>().unwrap(), key);
    let plain: MacroKey = "TIMEOUT".parse().unwrap();
    assert_eq!(plain.arity, None);
}

#[test]
fn an_object_like_key_sorts_before_the_function_like_ones() {
    let name = MacroName::new("QUEUE").unwrap();
    let nullary = MacroKey::object_like(name.clone());
    let unary = MacroKey::function_like(name, Arity::new(1));
    assert!(nullary < unary);
}

#[test]
fn merging_equal_definitions_keeps_one() {
    let key = MacroKey::object_like(MacroName::new("TIMEOUT").unwrap());
    let text = |s: &str| MacroDecl {
        key: key.clone(),
        body: MacroBody::Text(NormalisedMacroBody::from_source(s)),
    };
    assert_eq!(text("1").merge(text("1")), text("1"));
}

#[test]
fn merging_different_definitions_is_conditional() {
    let key = MacroKey::object_like(MacroName::new("TIMEOUT").unwrap());
    let text = |s: &str| MacroDecl {
        key: key.clone(),
        body: MacroBody::Text(NormalisedMacroBody::from_source(s)),
    };
    assert_eq!(text("1").merge(text("2")).body, MacroBody::Conditional);
}

#[test]
fn only_string_literals_are_string_literal_bodies() {
    let is = |source: &str| NormalisedMacroBody::from_source(source).is_string_literal();
    assert!(is("\"2007-2026\""));
    assert!(is("\"Copyright \" \"Broadcom\""));
    assert!(is("\"say \\\"hi\\\"\""));
    assert!(!is("256"));
    assert!(!is("\"a\" ++ Suffix"));
    assert!(!is("\"unterminated"));
    assert!(!is("%empty%"));
}
