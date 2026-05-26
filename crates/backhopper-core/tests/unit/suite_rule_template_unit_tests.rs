// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use regex::Regex;

use backhopper_core::suites::rules::{expand_template, validate_template_placeholders};

#[test]
fn expand_substitutes_named_captures() {
    let re = Regex::new(r"^deps/(?P<plugin>[^/]+)/priv/schema/.*\.schema$").unwrap();
    let caps = re
        .captures("deps/rabbitmq_web_amqp/priv/schema/foo.schema")
        .unwrap();
    let out = expand_template("{plugin}_config_schema_SUITE", &caps).unwrap();
    assert_eq!(out, "rabbitmq_web_amqp_config_schema_SUITE");
}

#[test]
fn expand_passes_through_literal_text() {
    let re = Regex::new(r"^(?P<a>x)$").unwrap();
    let caps = re.captures("x").unwrap();
    let out = expand_template("static_text_{a}_more_text", &caps).unwrap();
    assert_eq!(out, "static_text_x_more_text");
}

#[test]
fn expand_errors_on_unknown_placeholder() {
    let re = Regex::new(r"^(?P<a>x)$").unwrap();
    let caps = re.captures("x").unwrap();
    let err = expand_template("{nope}", &caps).unwrap_err();
    assert_eq!(err.placeholder, "nope");
}

#[test]
fn validate_passes_for_known_placeholders() {
    let allowed = vec!["plugin".to_owned(), "dir".to_owned()];
    assert!(validate_template_placeholders("{plugin}_SUITE", &allowed).is_ok());
    assert!(validate_template_placeholders("{dir}_{plugin}_SUITE", &allowed).is_ok());
    assert!(validate_template_placeholders("no_placeholders", &allowed).is_ok());
}

#[test]
fn validate_rejects_unknown_placeholders() {
    let allowed = vec!["plugin".to_owned()];
    let err = validate_template_placeholders("{plugin}_{typo}", &allowed).unwrap_err();
    assert_eq!(err.placeholder, "typo");
}

#[test]
fn validate_ignores_unbalanced_brace_at_end() {
    let allowed = vec!["plugin".to_owned()];
    // The current grammar treats a trailing `{` with no closing `}` as literal.
    assert!(validate_template_placeholders("{plugin}_trailing{", &allowed).is_ok());
}
