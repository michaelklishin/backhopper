// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_cuttlefish::{FragmentKind, parse_schema};

const OAUTH2_FIXTURE: &str = include_str!("../fixtures/oauth2.schema");

fn p() -> &'static Path {
    Path::new("test.schema")
}

#[test]
fn parses_translation_validator_and_mapping_from_real_schema() {
    let frags = parse_schema(OAUTH2_FIXTURE, p()).unwrap();
    assert_eq!(frags.len(), 3);
    let kinds: Vec<FragmentKind> = frags.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        vec![
            FragmentKind::Translation,
            FragmentKind::Mapping,
            FragmentKind::Validator,
        ]
    );
}

#[test]
fn translation_body_is_extracted_and_contains_the_mfa_call() {
    let frags = parse_schema(OAUTH2_FIXTURE, p()).unwrap();
    let t = frags
        .iter()
        .find(|f| f.kind == FragmentKind::Translation)
        .unwrap();
    let body = t.erlang_body.as_ref().expect("body present");
    assert!(
        body.contains("rabbit_cuttlefish:optionally_tagged_string"),
        "body: {body}"
    );
    assert!(t.body_start_line >= t.start_line);
}

#[test]
fn validator_body_extraction_skips_string_arguments() {
    let frags = parse_schema(OAUTH2_FIXTURE, p()).unwrap();
    let v = frags
        .iter()
        .find(|f| f.kind == FragmentKind::Validator)
        .unwrap();
    let body = v.erlang_body.as_ref().expect("body present");
    assert!(body.contains("is_tagged_binary"), "body: {body}");
    assert!(
        !body.contains("must be"),
        "body should be Erlang only, not the message string: {body}"
    );
}

#[test]
fn mapping_has_no_erlang_body() {
    let frags = parse_schema(OAUTH2_FIXTURE, p()).unwrap();
    let m = frags
        .iter()
        .find(|f| f.kind == FragmentKind::Mapping)
        .unwrap();
    assert!(m.erlang_body.is_none());
    assert_eq!(
        m.key.as_deref(),
        Some("rabbitmq_auth_backend_oauth2.preferred_username_claims")
    );
}

#[test]
fn comments_inside_schema_are_ignored() {
    let body = r#"
%% {translation, "fake", fun(X) -> ok end}.
{mapping, "real.key", "real.path", []}.
"#;
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].kind, FragmentKind::Mapping);
}

#[test]
fn braces_inside_strings_do_not_confuse_brace_counter() {
    let body = r#"
{translation, "k", fun(C) -> rabbit_cuttlefish:tag("{not a brace}", C) end}.
"#;
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags.len(), 1);
    let body = frags[0].erlang_body.as_ref().unwrap();
    assert!(body.contains("rabbit_cuttlefish:tag"));
}

#[test]
fn empty_input_returns_no_fragments() {
    let frags = parse_schema("", p()).unwrap();
    assert!(frags.is_empty());
}

#[test]
fn unrelated_top_level_tuples_are_skipped() {
    let body = r#"
{some_other_directive, foo, bar}.
{mapping, "x", "y", []}.
"#;
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0].kind, FragmentKind::Mapping);
}

#[test]
fn fun_body_with_nested_case_end_is_kept_intact() {
    // The call we care about (`after_inner_end:f`) sits *after* a nested
    // `case ... end`, so a parser that stops at the first `end` keyword would
    // miss it.
    let body = r#"
{translation, "k",
 fun(Conf) ->
     V = case rabbit_cuttlefish:opt(Conf) of
             {ok, X} -> X;
             error   -> none
         end,
     after_inner_end:f(V)
 end}.
"#;
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags.len(), 1);
    let body = frags[0].erlang_body.as_ref().unwrap();
    assert!(
        body.contains("after_inner_end:f"),
        "body should reach the call after the nested `case ... end`, got: {body}"
    );
}

#[test]
fn erlang_char_literal_inside_body_does_not_confuse_brace_counter() {
    // `${` and `$}` are character literals in Erlang. The parser must skip
    // the next character after `$` so the `{` and `}` do not affect the
    // brace stack.
    let body = r#"
{translation, "k",
 fun(Conf) ->
     case rabbit_cuttlefish:sep(Conf) of
         ${ -> open;
         $} -> close;
         _  -> rabbit_cuttlefish:fallback()
     end
 end}.
"#;
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags.len(), 1, "got: {frags:?}");
    let body = frags[0].erlang_body.as_ref().unwrap();
    assert!(body.contains("rabbit_cuttlefish:fallback"), "body: {body}");
}

#[test]
fn unbalanced_closing_brace_returns_parse_error() {
    let err = parse_schema("} {mapping, \"a\", \"b\", []}.", p()).unwrap_err();
    assert!(
        err.message.contains("unbalanced"),
        "expected unbalanced error, got: {}",
        err.message
    );
}

#[test]
fn start_line_tracks_the_opening_brace_position() {
    let body = "\n\n\n{mapping, \"a\", \"b\", []}.\n";
    let frags = parse_schema(body, p()).unwrap();
    assert_eq!(frags[0].start_line, 4);
}
