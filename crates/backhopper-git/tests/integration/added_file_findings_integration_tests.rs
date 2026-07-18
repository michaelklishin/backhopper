// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end checks for `compat::added_file::analyse_added_file`
//! and `analyse_added_files` against a real `TargetTreeIndex`. Backs
//! the CLI wiring at `commands/target_repo::collect_added_file_findings`
//! and the merge into `SeriesEvaluation`.

use backhopper_core::compat::added_file::{analyse_added_file, analyse_added_files};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
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

fn rabbit_globs() -> Vec<String> {
    vec![
        "deps/*/test".to_owned(),
        "deps/*/src".to_owned(),
        "deps/rabbitmq_ct_helpers/src".to_owned(),
    ]
}

#[test]
fn suite_with_missing_helper_emits_test_module_symbol_missing_and_diagnostic() {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/src/rabbit_app.erl", "-module(rabbit_app).\n");
    repo.commit("rabbit only");
    let target = build_target(&repo);
    let content = "-module(x_SUITE).\n\
                   f() -> amqp_utils:connection_config(1).\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/test/x_SUITE.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert!(
        findings
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::TestModuleSymbolMissing { .. }))
    );
    let suite_entry = findings
        .missing_test_modules
        .get(&rp("deps/rabbit/test/x_SUITE.erl"))
        .unwrap();
    assert_eq!(suite_entry.len(), 1);
}

#[test]
fn behaviour_module_missing_emitted_for_unresolved_attribute() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(some_module).\n-behaviour(custom_ct_hook).\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/some_module.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert!(
        findings
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourModuleMissing { .. }))
    );
}

#[test]
fn header_file_missing_emitted_for_unresolved_include_lib() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(x).\n-include_lib(\"khepri/include/khepri.hrl\").\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/x.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    let header_reasons: Vec<_> = findings
        .reasons
        .iter()
        .filter(|r| matches!(r, Reason::HeaderFileMissing { .. }))
        .collect();
    assert_eq!(header_reasons.len(), 1);
}

#[test]
fn behaviour_resolved_against_target_does_not_fire() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbit/src/custom_ct_hook.erl",
        "-module(custom_ct_hook).\n",
    );
    repo.commit("hook");
    let target = build_target(&repo);
    let content = "-module(x).\n-behaviour(custom_ct_hook).\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/x.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert!(
        !findings
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::BehaviourModuleMissing { .. }))
    );
}

#[test]
fn stdlib_behaviour_never_flagged() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(x).\n-behaviour(gen_server).\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/x.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert!(findings.reasons.is_empty());
}

#[test]
fn hrl_file_only_runs_include_resolver() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "%% header\n-include(\"missing.hrl\").\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/include/x.hrl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert_eq!(findings.reasons.len(), 1);
    assert!(matches!(
        findings.reasons[0],
        Reason::HeaderFileMissing { .. }
    ));
}

#[test]
fn analyse_added_files_merges_per_file_diagnostics() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let a = (
        rp("deps/rabbit/test/a_SUITE.erl"),
        "f() -> helper_one:x(1).\n".to_owned(),
    );
    let b = (
        rp("deps/rabbit/test/b_SUITE.erl"),
        "f() -> helper_two:y(2).\n".to_owned(),
    );
    let findings = analyse_added_files(
        [&a, &b].into_iter().map(|(p, c)| (p, c.as_str())),
        &target,
        &rabbit_globs(),
    );
    assert_eq!(findings.missing_test_modules.len(), 2);
    let test_module_reasons = findings
        .reasons
        .iter()
        .filter(|r| matches!(r, Reason::TestModuleSymbolMissing { .. }))
        .count();
    assert_eq!(test_module_reasons, 2);
}

#[test]
fn stdlib_include_lib_not_flagged_even_when_absent() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(x).\n\
                   -include_lib(\"kernel/include/file.hrl\").\n\
                   -include_lib(\"eunit/include/eunit.hrl\").\n\
                   -include_lib(\"common_test/include/ct.hrl\").\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/x.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    let header_count = findings
        .reasons
        .iter()
        .filter(|r| matches!(r, Reason::HeaderFileMissing { .. }))
        .count();
    assert_eq!(header_count, 0);
}

#[test]
fn relative_include_still_flagged_even_when_app_name_matches_stdlib() {
    // -include("kernel.hrl") is project-local: the stdlib-include-lib filter
    // must not apply to relative includes.
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(x).\n-include(\"kernel.hrl\").\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/x.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    assert!(
        findings
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::HeaderFileMissing { .. }))
    );
}

#[test]
fn non_erl_non_hrl_path_short_circuits_every_resolver() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "% looks like Erlang in a .md file\n\
                   -module(fake).\n\
                   -behaviour(missing).\n\
                   -include(\"missing.hrl\").\n\
                   f() -> some_helper:call(1).\n";
    let findings = analyse_added_file(&rp("docs/x.md"), content, &target, &rabbit_globs());
    assert!(findings.reasons.is_empty());
    assert!(findings.missing_test_modules.is_empty());
}

#[test]
fn non_suite_erl_does_not_run_test_helper_resolver() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/x.md", "x\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let content = "-module(broker_app).\nf() -> helper:x(1).\n";
    let findings = analyse_added_file(
        &rp("deps/rabbit/src/broker_app.erl"),
        content,
        &target,
        &rabbit_globs(),
    );
    let test_module_reasons = findings
        .reasons
        .iter()
        .filter(|r| matches!(r, Reason::TestModuleSymbolMissing { .. }))
        .count();
    assert_eq!(test_module_reasons, 0);
    assert!(findings.missing_test_modules.is_empty());
}
