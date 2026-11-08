// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::macro_decl::{MacroBody, MacroKey};
use backhopper_core::model::names::{ApplicationName, Arity, RelativePath};
use backhopper_core::model::snapshot::{HeaderRef, HrlFile};
use backhopper_erlang::ErlangExtractor;

fn extract(path: &str, app: &str, source: &str) -> HrlFile {
    ErlangExtractor::default().extract_header_file(
        RelativePath::new(path).unwrap(),
        ApplicationName::new(app).unwrap(),
        source,
    )
}

fn rabbit_common(source: &str) -> HrlFile {
    extract(
        "deps/rabbit_common/include/rabbit.hrl",
        "rabbit_common",
        source,
    )
}

fn body_of(hrl: &HrlFile, key: &str) -> MacroBody {
    let key: MacroKey = key.parse().unwrap();
    hrl.defines
        .iter()
        .find(|d| d.key == key)
        .unwrap_or_else(|| panic!("{key} not recorded in {:?}", hrl.defines))
        .body
        .clone()
}

fn text(body: &MacroBody) -> &str {
    match body {
        MacroBody::Text(t) => t.as_str(),
        MacroBody::Conditional => panic!("expected a text body"),
    }
}

fn header_ref(app: &str, app_path: &str) -> HeaderRef {
    HeaderRef {
        app: ApplicationName::new(app).unwrap(),
        app_path: RelativePath::new(app_path).unwrap(),
    }
}

#[test]
fn an_object_like_define_is_recorded_without_an_arity() {
    let hrl = rabbit_common("-define(DEFAULT_CONSUMER_TIMEOUT, 1800000).\n");
    let body = body_of(&hrl, "DEFAULT_CONSUMER_TIMEOUT");
    assert_eq!(text(&body), "1800000");
    assert_eq!(hrl.defines[0].key.arity, None);
}

#[test]
fn function_like_defines_are_recorded_with_their_arity() {
    let hrl = rabbit_common(
        "-define(IS_QUEUE(Q), is_record(Q, amqqueue)).\n-define(PAIR(A, B), {A, B}).\n",
    );
    let arities: Vec<_> = hrl.defines.iter().map(|d| d.key.arity).collect();
    assert_eq!(arities, [Some(Arity::new(1)), Some(Arity::new(2))]);
    assert_eq!(text(&body_of(&hrl, "IS_QUEUE/1")), "is_record(Q, amqqueue)");
}

#[test]
fn a_nullary_and_a_function_like_macro_of_one_name_are_two_keys() {
    let hrl = rabbit_common("-define(LOG, 1).\n-define(LOG(X), X).\n");
    assert_eq!(hrl.defines.len(), 2);
}

#[test]
fn a_define_under_ifdef_test_is_not_recorded() {
    let hrl =
        rabbit_common("-ifdef(TEST).\n-define(TEST_ONLY, 1).\n-endif.\n-define(PRODUCTION, 2).\n");
    let names: Vec<_> = hrl.defines.iter().map(|d| d.key.to_string()).collect();
    assert_eq!(names, ["PRODUCTION"]);
}

#[test]
fn a_define_under_ifndef_test_is_production_text() {
    let hrl =
        rabbit_common("-ifndef(TEST).\n-define(LIMIT, 10).\n-else.\n-define(LIMIT, 1).\n-endif.\n");
    assert_eq!(text(&body_of(&hrl, "LIMIT")), "10");
}

#[test]
fn a_define_in_both_branches_of_a_non_test_ifdef_is_one_conditional_entry() {
    let hrl = rabbit_common(
        "-ifdef(OTP_RELEASE).\n-define(WITH_STACKTRACE(C, R, S), C:R:S ->).\n-else.\n-define(WITH_STACKTRACE(C, R, S), C:R -> S = erlang:get_stacktrace(),).\n-endif.\n",
    );
    assert_eq!(hrl.defines.len(), 1);
    assert_eq!(body_of(&hrl, "WITH_STACKTRACE/3"), MacroBody::Conditional);
}

#[test]
fn a_define_only_inside_an_unrelated_ifdef_is_conditional() {
    let hrl = rabbit_common("-ifdef(FOO).\n-define(BAR, 1).\n-endif.\n");
    assert_eq!(body_of(&hrl, "BAR"), MacroBody::Conditional);
}

#[test]
fn a_define_under_an_if_expression_is_conditional() {
    let hrl = rabbit_common("-if(?OTP_RELEASE >= 27).\n-define(HAS_DOC, true).\n-endif.\n");
    assert_eq!(body_of(&hrl, "HAS_DOC"), MacroBody::Conditional);
}

#[test]
fn a_define_after_the_conditional_closes_is_text_again() {
    let hrl = rabbit_common("-ifdef(FOO).\n-define(A, 1).\n-endif.\n-define(B, 2).\n");
    assert_eq!(text(&body_of(&hrl, "B")), "2");
}

#[test]
fn a_nested_conditional_stays_conditional_until_every_frame_closes() {
    let hrl = rabbit_common(
        "-ifdef(FOO).\n-ifdef(BAR).\n-define(A, 1).\n-endif.\n-define(B, 2).\n-endif.\n-define(C, 3).\n",
    );
    assert_eq!(body_of(&hrl, "A"), MacroBody::Conditional);
    assert_eq!(body_of(&hrl, "B"), MacroBody::Conditional);
    assert_eq!(text(&body_of(&hrl, "C")), "3");
}

#[test]
fn a_conditional_definition_overrides_an_unconditional_one() {
    let hrl = rabbit_common("-define(A, 1).\n-ifdef(FOO).\n-define(A, 2).\n-endif.\n");
    assert_eq!(hrl.defines.len(), 1);
    assert_eq!(body_of(&hrl, "A"), MacroBody::Conditional);
}

#[test]
fn a_comment_inside_a_body_does_not_appear_in_it() {
    let hrl = rabbit_common("-define(LIMITS, #{max => 10, %% cap\n    min => 0}).\n");
    assert_eq!(text(&body_of(&hrl, "LIMITS")), "#{max => 10, min => 0}");
}

#[test]
fn reformatting_a_body_does_not_change_it() {
    let tight = rabbit_common("-define(PAIR, {a, b}).\n");
    let spread = rabbit_common("-define(PAIR,\n    {a,\n     b}).\n");
    assert_eq!(body_of(&tight, "PAIR"), body_of(&spread, "PAIR"));
}

#[test]
fn an_empty_define_body_is_recorded_as_the_empty_token() {
    let hrl = rabbit_common("-define(NOOP, ).\n");
    assert_eq!(text(&body_of(&hrl, "NOOP")), "%empty%");
}

#[test]
fn a_define_inside_a_comment_is_not_recorded() {
    let hrl = rabbit_common("%% -define(COMMENTED, 1).\n-define(REAL, 2).\n");
    assert_eq!(hrl.defines.len(), 1);
}

#[test]
fn defines_come_out_sorted_by_name_then_arity() {
    let hrl = rabbit_common("-define(B, 1).\n-define(A(X), X).\n-define(A, 0).\n");
    let keys: Vec<_> = hrl.defines.iter().map(|d| d.key.to_string()).collect();
    assert_eq!(keys, ["A", "A/1", "B"]);
}

#[test]
fn include_lib_is_recorded_as_app_and_app_relative_path() {
    let hrl = rabbit_common("-include_lib(\"rabbit_common/include/resource.hrl\").\n");
    assert_eq!(
        hrl.includes,
        [header_ref("rabbit_common", "include/resource.hrl")]
    );
}

#[test]
fn include_lib_of_another_app_keeps_that_app() {
    let hrl = rabbit_common("-include_lib(\"kernel/include/file.hrl\").\n");
    assert_eq!(hrl.includes, [header_ref("kernel", "include/file.hrl")]);
}

#[test]
fn a_relative_include_is_resolved_against_the_headers_own_directory() {
    let hrl = rabbit_common("-include(\"logging.hrl\").\n");
    assert_eq!(
        hrl.includes,
        [header_ref("rabbit_common", "include/logging.hrl")]
    );
}

#[test]
fn a_relative_include_with_a_parent_segment_stays_inside_the_app() {
    let hrl = extract(
        "deps/rabbit/include/internal/queues.hrl",
        "rabbit",
        "-include(\"../rabbit.hrl\").\n",
    );
    assert_eq!(hrl.includes, [header_ref("rabbit", "include/rabbit.hrl")]);
}

#[test]
fn a_relative_include_climbing_out_of_the_app_is_not_recorded() {
    let hrl = extract(
        "deps/rabbit/include/queues.hrl",
        "rabbit",
        "-include(\"../../../elsewhere.hrl\").\n",
    );
    assert!(hrl.includes.is_empty());
}

#[test]
fn an_include_naming_a_macro_is_not_recorded() {
    let hrl = rabbit_common("-include(\"?HEADER_DIR/x.hrl\").\n");
    assert!(hrl.includes.is_empty());
}

#[test]
fn an_include_under_a_non_test_conditional_is_recorded() {
    let hrl = rabbit_common(
        "-ifdef(OTP_RELEASE).\n-include_lib(\"kernel/include/logger.hrl\").\n-endif.\n",
    );
    assert_eq!(hrl.includes, [header_ref("kernel", "include/logger.hrl")]);
}

#[test]
fn an_include_under_ifdef_test_is_not_recorded() {
    let hrl = rabbit_common("-ifdef(TEST).\n-include_lib(\"eunit/include/eunit.hrl\").\n-endif.\n");
    assert!(hrl.includes.is_empty());
}

#[test]
fn the_header_carries_its_app() {
    let hrl = rabbit_common("");
    assert_eq!(hrl.app.as_ref().map(|a| a.as_str()), Some("rabbit_common"));
    assert_eq!(hrl.app_path().unwrap().as_str(), "include/rabbit.hrl");
}

#[test]
fn defines_inside_an_include_guard_are_text() {
    let hrl = rabbit_common(
        "-ifndef(RABBIT_HRL).\n-define(RABBIT_HRL, true).\n-define(DEFAULT_CONSUMER_TIMEOUT, 1800000).\n-endif.\n",
    );
    assert_eq!(text(&body_of(&hrl, "DEFAULT_CONSUMER_TIMEOUT")), "1800000");
    assert_eq!(text(&body_of(&hrl, "RABBIT_HRL")), "true");
}

#[test]
fn a_conditional_nested_in_an_include_guard_stays_conditional() {
    let hrl = rabbit_common(
        "-ifndef(RABBIT_HRL).\n-define(RABBIT_HRL, true).\n-ifdef(OTP_RELEASE).\n-define(HAS_DOC, true).\n-endif.\n-define(LIMIT, 1).\n-endif.\n",
    );
    assert_eq!(body_of(&hrl, "HAS_DOC"), MacroBody::Conditional);
    assert_eq!(text(&body_of(&hrl, "LIMIT")), "1");
}

#[test]
fn an_ifndef_whose_first_form_is_not_its_own_define_is_conditional() {
    let hrl = rabbit_common("-ifndef(LIMIT).\n-define(OTHER, 2).\n-define(LIMIT, 1).\n-endif.\n");
    assert_eq!(body_of(&hrl, "OTHER"), MacroBody::Conditional);
    assert_eq!(body_of(&hrl, "LIMIT"), MacroBody::Conditional);
}
