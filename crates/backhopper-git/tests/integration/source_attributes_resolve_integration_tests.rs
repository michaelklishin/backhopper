// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end checks for `compat::source_attributes::{
//! behaviour_resolves, resolve_include}` against a real
//! `TargetTreeIndex`. Backs candidates 5 (`HeaderFileMissing`) and 6
//! (`BehaviourModuleMissing`).

use backhopper_core::compat::source_attributes::{behaviour_resolves, resolve_include};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::model::names::{GitRef, ModuleName, RelativePath};
use backhopper_core::model::verdict::IncludeDirective;
use backhopper_git::{GitRepo, build_target_tree_index};

use backhopper_test_support::GitRepoFixture;

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn mn(s: &str) -> ModuleName {
    ModuleName::new(s).unwrap()
}

fn build_target(repo: &GitRepoFixture) -> TargetTreeIndex {
    let g = GitRepo::open(repo.dir.path()).unwrap();
    let head = GitRef::new("HEAD").unwrap();
    build_target_tree_index(&g, &head).unwrap()
}

#[test]
fn behaviour_present_under_wildcard_root_resolves() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbit/src/custom_ct_hook.erl",
        "-module(custom_ct_hook).\n",
    );
    repo.commit("with behaviour");
    let target = build_target(&repo);
    let globs = vec!["deps/*/src".to_owned()];
    assert!(behaviour_resolves(&target, &globs, &mn("custom_ct_hook")));
}

#[test]
fn behaviour_absent_does_not_resolve() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/README.md", "hi\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let globs = vec!["deps/*/src".to_owned()];
    assert!(!behaviour_resolves(&target, &globs, &mn("custom_ct_hook")));
}

#[test]
fn include_relative_to_source_dir_resolves() {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/test/x.hrl", "%% header\n");
    repo.commit("hdr");
    let target = build_target(&repo);
    let directive = IncludeDirective::Include {
        path: "x.hrl".to_owned(),
    };
    let found = resolve_include(&target, &rp("deps/rabbit/test/x_SUITE.erl"), &directive).unwrap();
    assert_eq!(found.as_str(), "deps/rabbit/test/x.hrl");
}

#[test]
fn include_absent_returns_attempted_paths() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/README.md", "hi\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let directive = IncludeDirective::Include {
        path: "x.hrl".to_owned(),
    };
    let attempted =
        resolve_include(&target, &rp("deps/rabbit/test/x_SUITE.erl"), &directive).unwrap_err();
    assert_eq!(attempted.len(), 1);
    assert_eq!(attempted[0].as_str(), "deps/rabbit/test/x.hrl");
}

#[test]
fn include_lib_resolves_under_deps_app_path() {
    let repo = GitRepoFixture::new();
    repo.write_file(
        "deps/rabbitmq_amqp_client/include/amqp_client.hrl",
        "%% hdr\n",
    );
    repo.commit("hdr");
    let target = build_target(&repo);
    let directive = IncludeDirective::IncludeLib {
        path: "rabbitmq_amqp_client/include/amqp_client.hrl".to_owned(),
    };
    let found = resolve_include(&target, &rp("deps/rabbit/test/x_SUITE.erl"), &directive).unwrap();
    assert_eq!(
        found.as_str(),
        "deps/rabbitmq_amqp_client/include/amqp_client.hrl"
    );
}

#[test]
fn include_lib_absent_attempts_deps_apps_lib_in_order() {
    let repo = GitRepoFixture::new();
    repo.write_file("docs/README.md", "hi\n");
    repo.commit("docs");
    let target = build_target(&repo);
    let directive = IncludeDirective::IncludeLib {
        path: "rabbitmq_amqp_client/include/amqp_client.hrl".to_owned(),
    };
    let attempted =
        resolve_include(&target, &rp("deps/rabbit/test/x_SUITE.erl"), &directive).unwrap_err();
    assert_eq!(attempted.len(), 3);
    let s: Vec<&str> = attempted.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        s,
        vec![
            "deps/rabbitmq_amqp_client/include/amqp_client.hrl",
            "apps/rabbitmq_amqp_client/include/amqp_client.hrl",
            "lib/rabbitmq_amqp_client/include/amqp_client.hrl",
        ]
    );
}

#[test]
fn include_with_no_directory_lives_next_to_source() {
    let repo = GitRepoFixture::new();
    repo.write_file("deps/rabbit/include/api.hrl", "%% hdr\n");
    repo.commit("hdr");
    let target = build_target(&repo);
    let directive = IncludeDirective::Include {
        path: "api.hrl".to_owned(),
    };
    // The source is in include/, so relative resolution lands at deps/rabbit/include/api.hrl.
    let found = resolve_include(&target, &rp("deps/rabbit/include/other.hrl"), &directive).unwrap();
    assert_eq!(found.as_str(), "deps/rabbit/include/api.hrl");
}
