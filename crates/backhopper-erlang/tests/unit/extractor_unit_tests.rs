// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::model::snapshot::{ArityMatch, Visibility};
use backhopper_erlang::ErlangExtractor;

const SOURCE_RA: &str = r#"
%% @hidden
-module(ra_internal).
-export([f/1, g/2]).
f(X) -> X.
g(X, Y) -> {X, Y}.
"#;

const SOURCE_PUBLIC: &str = r#"
-module(ra).
-behaviour(gen_statem).
-export([init/1, process_command/2, process_command/3]).
-export_type([range/0]).
-callback init(Conf :: term()) -> state().
-spec process_command(A, B) -> ok | {error, term()}.
-type range() :: undefined | {ra:index(), ra:index()}.
init(Conf) -> Conf.
process_command(_A, _B) -> ok.
process_command(_A, _B, _T) -> ok.
"#;

const SOURCE_TEST_ONLY: &str = r#"
-module(t).
-ifdef(TEST).
-export([test_helper/0]).
-endif.
-export([real/1]).
real(X) -> X.
"#;

const HEADER: &str = r#"
-type ra_index() :: non_neg_integer().
-record(cfg, {id :: ra_server_id(), uid :: ra_uid(), log_id :: term()}).
"#;

#[test]
fn extracts_public_module() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(SOURCE_PUBLIC).unwrap();
    assert_eq!(m.name.as_str(), "ra");
    assert_eq!(m.visibility, Visibility::Public);
    assert_eq!(m.behaviours.len(), 1);
    assert_eq!(m.exports.len(), 3);
    assert_eq!(m.export_types.len(), 1);
    assert_eq!(m.callbacks.len(), 1);
    assert_eq!(m.specs.len(), 1);
    assert_eq!(m.types.len(), 1);
}

#[test]
fn detects_hidden_module_via_doc_comment() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(SOURCE_RA).unwrap();
    assert_eq!(m.visibility, Visibility::Hidden);
}

#[test]
fn module_with_mix_of_test_and_real_exports_is_public() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(SOURCE_TEST_ONLY).unwrap();
    assert_eq!(m.visibility, Visibility::Public);
}

#[test]
fn module_with_only_test_exports_is_test_only() {
    let only_test = r#"
-module(t).
-ifdef(TEST).
-export([helper/0]).
-endif.
helper() -> ok.
"#;
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(only_test).unwrap();
    assert_eq!(m.visibility, Visibility::TestOnly);
}

#[test]
fn header_extraction_yields_records_and_types() {
    let ex = ErlangExtractor::default();
    let h = ex.extract_header_file("include/ra.hrl", HEADER);
    assert_eq!(h.types.len(), 1);
    assert_eq!(h.records.len(), 1);
    assert_eq!(h.records[0].name.as_str(), "cfg");
    assert_eq!(h.records[0].fields.len(), 3);
}

#[test]
fn extract_source_handles_mixed_files() {
    let ex = ErlangExtractor::default();
    let files = vec![
        (
            PathBuf::from("src/ra.erl"),
            SOURCE_PUBLIC.as_bytes().to_vec(),
        ),
        (PathBuf::from("include/ra.hrl"), HEADER.as_bytes().to_vec()),
    ];
    let result = ex.extract_source(files).unwrap();
    assert_eq!(result.modules.len(), 1);
    assert_eq!(result.headers.len(), 1);
}

// Taken from gen_batch_server: the behaviour declares its callback set,
// three of them optional, and an opaque type. All four attribute forms
// must land in the module surface.
const SOURCE_GEN_BATCH_SERVER: &str = r#"
-module(gen_batch_server).
-behaviour(gen_server).
-opaque request_id() :: gen:request_id().
-callback init(Args :: term()) -> {ok, State} | {stop, Reason :: term()}.
-callback handle_batch([op()], State) -> {ok, State} | {stop, Reason :: term()}.
-callback handle_continue(Continue :: term(), State) -> {ok, State}.
-callback terminate(Reason :: term(), State :: term()) -> term().
-callback format_status(State :: term()) -> term().
-optional_callbacks([handle_continue/2, format_status/1, terminate/2]).
"#;

#[test]
fn gen_batch_server_callbacks_optional_and_opaque() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(SOURCE_GEN_BATCH_SERVER).unwrap();
    assert_eq!(m.behaviours.len(), 1);
    assert_eq!(m.callbacks.len(), 5);
    assert_eq!(m.optional_callbacks.len(), 3);
    assert_eq!(m.opaques.len(), 1);
}

// A directory scan hands extract_source a file that forgot its -module
// attribute and a non-source file; both are skipped, only the well-formed
// module survives.
#[test]
fn extract_source_skips_non_erl_and_module_less_files() {
    let ex = ErlangExtractor::default();
    let files = vec![
        (
            PathBuf::from("src/ra_helpers.erl"),
            b"-export([noop/0]).\nnoop() -> ok.\n".to_vec(),
        ),
        (PathBuf::from("README.md"), b"# ra\n".to_vec()),
        (
            PathBuf::from("src/ra.erl"),
            SOURCE_PUBLIC.as_bytes().to_vec(),
        ),
    ];
    let result = ex.extract_source(files).unwrap();
    assert_eq!(result.modules.len(), 1);
}

// The three -deprecated shapes the extractor recognises: a whole-module
// deprecation, a function with any arity, and a function at one arity.
#[test]
fn extracts_the_three_deprecation_shapes() {
    let ex = ErlangExtractor::default();

    let module_wide = ex
        .extract_module("-module(ra_old).\n-deprecated(module).\n-export([f/0]).\nf() -> ok.\n")
        .unwrap();
    assert!(module_wide.deprecations[0].module_wide);

    let any_arity = ex
        .extract_module(
            "-module(ra_log).\n-deprecated([{write, '_'}]).\n\
             -export([write/1, write/2]).\nwrite(X) -> X.\nwrite(X, _) -> X.\n",
        )
        .unwrap();
    assert!(matches!(
        any_arity.deprecations[0].arity_match,
        ArityMatch::Any
    ));

    let exact = ex
        .extract_module(
            "-module(ra_log).\n-deprecated([{write, 2}]).\n-export([write/2]).\nwrite(X, _) -> X.\n",
        )
        .unwrap();
    assert!(matches!(
        exact.deprecations[0].arity_match,
        ArityMatch::Exact { .. }
    ));
}

// A version-gated module behind -if(...) / -endif: the conditional markers
// are consumed and the unconditional surface is still extracted.
#[test]
fn extracts_module_with_if_conditional_block() {
    let m = ErlangExtractor::default()
        .extract_module(
            "-module(ra_compat).\n\
             -if(?OTP_RELEASE >= 26).\n\
             -export([new_api/0]).\n\
             new_api() -> ok.\n\
             -endif.\n\
             -export([stable/0]).\n\
             stable() -> ok.\n",
        )
        .unwrap();
    assert_eq!(m.name.as_str(), "ra_compat");
    assert!(m.exports.iter().any(|e| e.name.as_str() == "stable"));
}

// A header file carrying an -opaque alongside a -record: both land in the
// header surface, the opaque kept distinct from the records.
#[test]
fn extract_header_file_collects_opaque_and_record() {
    let h = ErlangExtractor::default().extract_header_file(
        "include/ra.hrl",
        "-opaque ra_idxterm() :: term().\n-record(cfg, {id :: ra_server_id()}).\n",
    );
    assert_eq!(h.opaques.len(), 1);
    assert_eq!(h.records.len(), 1);
}
