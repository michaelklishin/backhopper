// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end checks for `TestSuiteFile<Parsed>::resolve` against a
//! real `TargetTreeIndex` built from a temp gix repo. Backs candidate
//! 1 (TestModuleSymbolMissing) and the helper-resolver glob semantics
//! in `compat::test_suite`.

use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::compat::test_suite::TestSuiteFile;
use backhopper_core::model::names::{GitRef, RelativePath};
use backhopper_core::model::verdict::Reason;
use backhopper_git::{GitRepo, build_target_tree_index};

use backhopper_test_support::GitRepoFixture;

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn build_target(repo: &GitRepoFixture) -> TargetTreeIndex {
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let head = GitRef::new("HEAD").unwrap();
    build_target_tree_index(&g, &head).unwrap()
}

#[test]
fn unresolved_helper_emits_test_module_symbol_missing() {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/src/rabbit_app.erl", "-module(rabbit_app).\n");
    repo.commit("rabbit only");
    let target = build_target(&repo);

    let src = "-module(amqp10_connection_max_SUITE).\n\
               connection_config_test() ->\n\
                   amqp_utils:connection_config(1).\n";
    let parsed = TestSuiteFile::new(
        rp("deps/rabbit/test/amqp10_connection_max_SUITE.erl"),
        src.to_owned(),
    )
    .parse()
    .unwrap();
    let globs = vec![
        "deps/*/test".to_owned(),
        "deps/*/src".to_owned(),
        "deps/rabbitmq_ct_helpers/src".to_owned(),
    ];
    let resolved = parsed.resolve(&target, &globs);
    let reasons = resolved.into_reasons();
    assert_eq!(reasons.len(), 1);
    match &reasons[0] {
        Reason::TestModuleSymbolMissing {
            suite_path,
            missing_module,
            call_sites,
        } => {
            assert_eq!(
                suite_path.as_str(),
                "deps/rabbit/test/amqp10_connection_max_SUITE.erl"
            );
            assert_eq!(missing_module.as_str(), "amqp_utils");
            assert_eq!(call_sites.len(), 1);
            assert_eq!(call_sites[0].function.as_str(), "connection_config");
            assert_eq!(call_sites[0].line, 3);
        }
        other => panic!("unexpected reason: {other:?}"),
    }
}

#[test]
fn helper_present_under_wildcard_root_is_resolved() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbit/test/amqp_utils.erl",
        "-module(amqp_utils).\n-export([connection_config/1]).\n\
         connection_config(_) -> ok.\n",
    );
    repo.write_file("deps/rabbit/src/rabbit_app.erl", "-module(rabbit_app).\n");
    repo.commit("with helper");
    let target = build_target(&repo);

    let src = "f() -> amqp_utils:connection_config(1).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let resolved = parsed.resolve(
        &target,
        &["deps/*/test".to_owned(), "deps/*/src".to_owned()],
    );
    assert!(resolved.missing_modules().is_empty());
}

#[test]
fn helper_present_under_literal_root_is_resolved() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_ct_helpers/src/rabbit_ct_broker_helpers.erl",
        "-module(rabbit_ct_broker_helpers).\n",
    );
    repo.commit("ct helpers");
    let target = build_target(&repo);

    let src = "f() -> rabbit_ct_broker_helpers:start(1).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let resolved = parsed.resolve(&target, &["deps/rabbitmq_ct_helpers/src".to_owned()]);
    assert!(resolved.missing_modules().is_empty());
}

#[test]
fn stdlib_modules_skipped_even_when_absent() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/README.md", "hi\n");
    repo.commit("docs only");
    let target = build_target(&repo);

    let src = "f() -> lists:reverse([1, 2, 3]), io:format(\"hi\").\n";
    let parsed = TestSuiteFile::new(rp("a/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let resolved = parsed.resolve(&target, &["deps/*/test".to_owned()]);
    assert!(resolved.missing_modules().is_empty());
}

#[test]
fn diagnostic_entry_captures_call_site_count() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("nothing");
    let target = build_target(&repo);

    let src = "f() -> amqp_utils:a(1), amqp_utils:b(2), other_helper:c(3).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let resolved = parsed.resolve(&target, &["deps/*/test".to_owned()]);
    let (suite, by_module) = resolved.into_diagnostic_entry().unwrap();
    assert_eq!(suite.as_str(), "deps/rabbit/test/x_SUITE.erl");
    assert_eq!(by_module.len(), 2);
    assert_eq!(
        by_module
            .iter()
            .map(|(k, v)| (k.as_str().to_owned(), *v))
            .collect::<Vec<_>>(),
        vec![("amqp_utils".to_owned(), 2), ("other_helper".to_owned(), 1)]
    );
}

#[test]
fn empty_globs_means_every_non_stdlib_module_is_missing() {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/src/rabbit_app.erl", "-module(rabbit_app).\n");
    repo.commit("rabbit");
    let target = build_target(&repo);

    let src = "f() -> rabbit_app:start(1).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    let resolved = parsed.resolve(&target, &[]);
    assert_eq!(resolved.missing_modules().len(), 1);
    assert_eq!(resolved.missing_modules()[0].module.as_str(), "rabbit_app");
}

#[test]
fn invalid_multi_wildcard_glob_is_silently_dropped() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("nothing");
    let target = build_target(&repo);

    let src = "f() -> some_mod:call(1).\n";
    let parsed = TestSuiteFile::new(rp("deps/rabbit/test/x_SUITE.erl"), src.to_owned())
        .parse()
        .unwrap();
    // Globs with more than one `*` are not supported and silently
    // skipped. The resolver should still answer correctly using the
    // remaining roots; here that means "every non-stdlib module is
    // missing" because no root matches.
    let resolved = parsed.resolve(
        &target,
        &[
            "deps/*/*/test".to_owned(),
            "deps/rabbitmq_ct_helpers/src".to_owned(),
        ],
    );
    assert_eq!(resolved.missing_modules().len(), 1);
}
