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
    let source = "-module(ra_server).\n-define(SERVER, ra_directory).\n-export([start/0]).\nstart() -> ok.\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("ra_server.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("ra_directory")
    );
}

#[test]
fn build_table_from_parameterized_define() {
    let source = "-define(LOG(L, M), logger:log(L, M)).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("ra_log.erl"), &files);
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
        "-define(SERVER, rabbit_amqqueue).\n".to_owned(),
    );
    let source = "-include(\"shared.hrl\").\n";
    let table = build_macro_table(
        source,
        &PathBuf::from("deps/rabbit/src/rabbit_channel.erl"),
        &files,
    );
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("rabbit_amqqueue")
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
    let table = build_macro_table(
        source,
        &PathBuf::from("deps/consumer/src/client.erl"),
        &files,
    );
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("shared_mod")
    );
}

#[test]
fn build_table_follows_transitive_includes() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/ra/src/ra.hrl"),
        "-include(\"ra_server.hrl\").\n".to_owned(),
    );
    files.insert(
        PathBuf::from("deps/ra/src/ra_server.hrl"),
        "-define(SERVER, ra_directory).\n".to_owned(),
    );
    let source = "-include(\"ra.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/ra/src/ra_log.erl"), &files);
    assert_eq!(
        table.get(&key("SERVER", None)).map(String::as_str),
        Some("ra_directory")
    );
}

#[test]
fn build_table_handles_include_cycle() {
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/ra/src/ra.hrl"),
        "-include(\"ra_server.hrl\").\n-define(MAGIC, ra).\n".to_owned(),
    );
    files.insert(
        PathBuf::from("deps/ra/src/ra_server.hrl"),
        "-include(\"ra.hrl\").\n".to_owned(),
    );
    let source = "-include(\"ra.hrl\").\n";
    let table = build_macro_table(source, &PathBuf::from("deps/ra/src/ra_log.erl"), &files);
    assert_eq!(
        table.get(&key("MAGIC", None)).map(String::as_str),
        Some("ra")
    );
}

#[test]
fn build_table_drops_unresolvable_includes_silently() {
    let source = "-include(\"missing.hrl\").\n-define(KEEP, ok).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("ra_directory.erl"), &files);
    assert_eq!(
        table.get(&key("KEEP", None)).map(String::as_str),
        Some("ok")
    );
}

#[test]
fn build_table_ignores_attributes_other_than_define_and_include() {
    let source = "-module(ra_machine).\n-export([start/0]).\n-behaviour(gen_server).\n";
    let files = FileMap::new();
    let table = build_macro_table(source, &PathBuf::from("ra_machine.erl"), &files);
    assert!(table.is_empty());
}

#[test]
fn build_table_does_not_panic_on_trailing_backslash_in_string_literal() {
    let source = "-define(BAD, \"trailing\\\n";
    let files = FileMap::new();
    let _ = build_macro_table(source, &PathBuf::from("ra_log.erl"), &files);
}

#[test]
fn build_table_does_not_panic_on_trailing_backslash_in_quoted_atom() {
    let source = "-define(BAD, 'ra\\";
    let files = FileMap::new();
    let _ = build_macro_table(source, &PathBuf::from("ra_log.erl"), &files);
}
