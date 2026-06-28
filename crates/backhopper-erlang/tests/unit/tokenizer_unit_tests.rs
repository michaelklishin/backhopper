// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::tokenizer::iterate_attributes;

#[test]
fn picks_up_module_and_export() {
    let src = "-module(ra).\n-export([init/1]).\n";
    let blocks = iterate_attributes(src);
    let names: Vec<_> = blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["module", "export"]);
}

#[test]
fn skips_line_comments_and_keeps_attributes() {
    let src = "%% top comment\n-module(ra). %% end\n%% mid\n-export([init/1]).\n";
    let blocks = iterate_attributes(src);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].name, "module");
    assert_eq!(blocks[1].name, "export");
}

#[test]
fn handles_multi_line_export_lists() {
    let src = "-export([\n  init/1,\n  apply/3,\n  process_command/2\n]).\n";
    let blocks = iterate_attributes(src);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].body.contains("init/1"));
    assert!(blocks[0].body.contains("apply/3"));
    assert!(blocks[0].body.contains("process_command/2"));
}

#[test]
fn handles_quoted_atoms_in_attribute_bodies() {
    let src = "-module(osiris_log).\n-record(state, {name :: 'tagged_value'}).\n";
    let blocks = iterate_attributes(src);
    assert_eq!(blocks.len(), 2);
    assert!(blocks[1].body.contains("'tagged_value'"));
}

#[test]
fn ignores_dash_minus_in_function_bodies() {
    let src = "-module(ra_lib).\nid(X) -> -X.\n-export([id/1]).\n";
    let blocks = iterate_attributes(src);
    let names: Vec<_> = blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["module", "export"]);
}

#[test]
fn char_literal_paren_does_not_close_attribute_body() {
    let src = "-define(SEP, $)).\n-export([init/1]).\n";
    let blocks = iterate_attributes(src);
    let names: Vec<_> = blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["define", "export"],
        "$) is a char literal; the inner ) must not close the -define"
    );
    assert!(
        blocks[0].body.contains("$)"),
        "define body must keep the char literal intact, got: {:?}",
        blocks[0].body
    );
}

#[test]
fn char_literal_dot_does_not_terminate_form() {
    let src = "-define(DOT, $.).\n-export([init/1]).\n";
    let blocks = iterate_attributes(src);
    let names: Vec<_> = blocks.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["define", "export"],
        "$. is a char literal; it must not terminate the form"
    );
}
