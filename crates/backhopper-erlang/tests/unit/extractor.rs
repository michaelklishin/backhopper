use std::path::PathBuf;

use backhopper_core::model::snapshot::Visibility;
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
fn marks_test_only_when_export_is_inside_ifdef_test() {
    let ex = ErlangExtractor::default();
    let m = ex.extract_module(SOURCE_TEST_ONLY).unwrap();
    assert!(matches!(
        m.visibility,
        Visibility::TestOnly | Visibility::Public
    ));
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
