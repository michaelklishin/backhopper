use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::SuiteMatcher;
use backhopper_xref_reader::AstSuiteMatcher;

fn write(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn m(name: &str) -> ModuleName {
    ModuleName::new(name).unwrap()
}

#[test]
fn ast_matcher_resolves_explicit_remote_call() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "vhost_SUITE.erl",
        "-module(vhost_SUITE).\n\
         -export([go/0]).\n\
         go() -> rabbit_db:set(1, 2).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(refs.contains(&m("rabbit_db")));
    assert!(matcher.parsed_cleanly(&p));
}

#[test]
fn ast_matcher_resolves_import_only_reference() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -import(rabbit_db, [set/2]).\n\
         -export([go/0]).\n\
         go() -> set(1, 2).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(
        refs.contains(&m("rabbit_db")),
        "the imported module counts as a reference even without a `mod:` prefix"
    );
}

#[test]
fn ast_matcher_resolves_module_macro_self_call() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "vhost_SUITE.erl",
        "-module(vhost_SUITE).\n\
         -export([go/0]).\n\
         go() -> ?MODULE:helper(1).\n\
         helper(X) -> X.\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("vhost_SUITE")]);
    assert!(refs.contains(&m("vhost_SUITE")));
}

#[test]
fn ast_matcher_resolves_apply_with_atom_literals() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "spawn_SUITE.erl",
        "-module(spawn_SUITE).\n\
         -export([go/0]).\n\
         go() -> spawn(worker, init, [config]).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("worker")]);
    assert!(refs.contains(&m("worker")));
}

#[test]
fn ast_matcher_ignores_match_in_string_literal() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -export([go/0]).\n\
         go() -> io:format(\"call rabbit_db:set/2 maybe~n\", []).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(
        !refs.contains(&m("rabbit_db")),
        "string-literal mention is a substring false positive that AST rejects"
    );
    assert!(matcher.parsed_cleanly(&p));
}

#[test]
fn ast_matcher_ignores_match_in_comment() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -export([go/0]).\n\
         %% rabbit_db:set/2 is no longer called here\n\
         go() -> ok.\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(!refs.contains(&m("rabbit_db")));
}

#[test]
fn ast_matcher_falls_back_to_substring_when_module_attr_missing() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "no_module_SUITE.erl",
        "%% missing -module/1 attribute\n\
         go() -> rabbit_db:set(1).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(
        refs.contains(&m("rabbit_db")),
        "substring fallback should still catch the call"
    );
    assert!(!matcher.parsed_cleanly(&p));
}

#[test]
fn ast_matcher_returns_empty_for_unrelated_modules() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -export([go/0]).\n\
         go() -> rabbit_db:set(1).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(&p, &[m("never_called")]);
    assert!(refs.is_empty());
}

#[test]
fn ast_matcher_returns_referenced_subset_of_triggering_modules() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -export([go/0]).\n\
         go() -> rabbit_db:set(1), khepri:put(2).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs =
        matcher.modules_referenced_in_suite(&p, &[m("rabbit_db"), m("never_called"), m("khepri")]);
    assert_eq!(refs.len(), 2);
    assert!(refs.contains(&m("rabbit_db")));
    assert!(refs.contains(&m("khepri")));
    assert!(!refs.contains(&m("never_called")));
}

#[test]
fn ast_matcher_caches_parsed_state_per_path() {
    let dir = TempDir::new().unwrap();
    let p = write(
        &dir,
        "x_SUITE.erl",
        "-module(x_SUITE).\n\
         -export([go/0]).\n\
         go() -> rabbit_db:set(1).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let _ = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    std::fs::remove_file(&p).unwrap();
    let second = matcher.modules_referenced_in_suite(&p, &[m("rabbit_db")]);
    assert!(
        second.contains(&m("rabbit_db")),
        "second call should use the cached parse"
    );
}

#[test]
fn ast_matcher_returns_empty_for_missing_file() {
    let mut matcher = AstSuiteMatcher::new();
    let refs = matcher.modules_referenced_in_suite(
        &PathBuf::from("/does/not/exist_SUITE.erl"),
        &[m("rabbit_db")],
    );
    assert!(refs.is_empty());
    assert!(!matcher.parsed_cleanly(&PathBuf::from("/does/not/exist_SUITE.erl")));
}

#[test]
fn ast_matcher_keeps_per_suite_results_separate() {
    let dir = TempDir::new().unwrap();
    let a = write(
        &dir,
        "a_SUITE.erl",
        "-module(a_SUITE).\n\
         -export([go/0]).\n\
         go() -> rabbit_db:set(1).\n",
    );
    let b = write(
        &dir,
        "b_SUITE.erl",
        "-module(b_SUITE).\n\
         -export([go/0]).\n\
         go() -> khepri:put(2).\n",
    );
    let mut matcher = AstSuiteMatcher::new();
    let refs_a = matcher.modules_referenced_in_suite(&a, &[m("rabbit_db"), m("khepri")]);
    let refs_b = matcher.modules_referenced_in_suite(&b, &[m("rabbit_db"), m("khepri")]);
    assert_eq!(refs_a, [m("rabbit_db")].into_iter().collect());
    assert_eq!(refs_b, [m("khepri")].into_iter().collect());
}
