// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::compat::source_macros::{FileMap, build_macro_table};
use backhopper_core::erlang_macros::MacroKey;

fn key(name: &str, arity: Option<u8>) -> MacroKey {
    MacroKey {
        name: name.to_owned(),
        arity,
    }
}

#[test]
fn build_table_from_inline_define() {
    let source = "-module(m).\n-define(SERVER, my_module).\n-export([go/0]).\ngo() -> ok.\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("m.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("my_module")
    );
}

#[test]
fn build_table_from_parameterized_define() {
    let source = "-define(LOG(L, M), logger:log(L, M)).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("m.erl"), &files);
    assert_eq!(
        table.get(&key("LOG", Some(2))).map(String::as_str),
        Some("logger:log(L, M)")
    );
}

#[test]
fn build_table_follows_relative_include() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/rabbit/src/shared.hrl"),
        "-define(SERVER, my_module).\n".to_owned(),
    );
    let source = "-include(\"shared.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/rabbit/src/m.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("my_module")
    );
}

#[test]
fn build_table_follows_include_lib_via_app_root() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/shared/include/api.hrl"),
        "-define(SERVER, shared_mod).\n".to_owned(),
    );
    let source = "-include_lib(\"shared/include/api.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/consumer/src/m.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("shared_mod")
    );
}

#[test]
fn build_table_follows_transitive_includes() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/r/src/a.hrl"),
        "-include(\"b.hrl\").\n".to_owned(),
    );
    files.insert(
        PathBuf::from("deps/r/src/b.hrl"),
        "-define(SERVER, deep_mod).\n".to_owned(),
    );
    let source = "-include(\"a.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/r/src/m.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("deep_mod")
    );
}

#[test]
fn build_table_handles_include_cycle() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/r/src/a.hrl"),
        "-include(\"b.hrl\").\n-define(A, alpha).\n".to_owned(),
    );
    files.insert(
        PathBuf::from("deps/r/src/b.hrl"),
        "-include(\"a.hrl\").\n".to_owned(),
    );
    let source = "-include(\"a.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/r/src/m.erl"), &files);
    assert_eq!(
        table.get(&key("A", None)).map(String::as_str),
        Some("alpha")
    );
}

#[test]
fn build_table_drops_unresolvable_includes_silently() {
    let source = "-include(\"missing.hrl\").\n-define(KEEP, ok).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("m.erl"), &files);
    assert_eq!(
        table.get(&key("KEEP", None)).map(String::as_str),
        Some("ok")
    );
}

#[test]
fn build_table_ignores_attributes_other_than_define_and_include() {
    let source = "-module(m).\n-export([go/0]).\n-behaviour(gen_server).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("m.erl"), &files);
    assert!(table.is_empty());
}

#[test]
fn build_table_does_not_panic_on_trailing_backslash_in_string_literal() {
    let source = "-define(BAD, \"trailing\\\n";
    let files = FileMap::new();
    let _ = build_macro_table(source, &PathBuf::from("m.erl"), &files);
}

#[test]
fn build_table_does_not_panic_on_trailing_backslash_in_quoted_atom() {
    let source = "-define(BAD, 'foo\\";
    let files = FileMap::new();
    let _ = build_macro_table(source, &PathBuf::from("m.erl"), &files);
}
