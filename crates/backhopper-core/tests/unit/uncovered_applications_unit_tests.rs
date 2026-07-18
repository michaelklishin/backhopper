// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::app_src::AppSrcSpec;
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::{ExtraRule, ExtraRuleTrigger, PlanInput, SuiteRefSpec, plan};

fn touch(dir: &std::path::Path, rel: &str, body: &str) -> PathBuf {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, body).unwrap();
    full
}

fn app(name: &str, root: &std::path::Path, modules: Vec<&str>) -> AppSrcSpec {
    AppSrcSpec {
        name: ApplicationName::new(name).unwrap(),
        path: root.join(format!("deps/{name}/src/{name}.app.src")),
        vsn: None,
        modules: modules
            .into_iter()
            .map(|m| ModuleName::new(m).unwrap())
            .collect(),
        applications: BTreeSet::new(),
    }
}

fn input(root: &std::path::Path, modified: Vec<&str>, apps: Vec<AppSrcSpec>) -> PlanInput {
    PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: modified.into_iter().map(PathBuf::from).collect(),
        apps,
        library_apps: Vec::new(),
        extra_rules: Vec::new(),
        implementer_index: BTreeMap::new(),
        dep_module_index: BTreeMap::new(),
    }
}

// The suite exercises handlers over HTTP and references no modified module: no built-in rule selects it.
#[test]
fn protocol_boundary_app_is_reported_uncovered_with_candidates() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "deps/mgmt/src/mgmt_util.erl", "-module(mgmt_util).\n");
    touch(
        root,
        "deps/mgmt/test/mgmt_http_SUITE.erl",
        "-module(mgmt_http_SUITE).\nall() -> [].\n",
    );
    let apps = vec![app("mgmt", root, vec!["mgmt_util"])];
    let p = plan(&input(root, vec!["deps/mgmt/src/mgmt_util.erl"], apps));
    assert!(p.entries.is_empty());
    assert_eq!(p.uncovered.len(), 1);
    let u = &p.uncovered[0];
    assert_eq!(u.application.as_str(), "mgmt");
    assert_eq!(u.modified_modules, 1);
    assert_eq!(u.discovered_suites, 1);
    assert_eq!(u.suites[0].as_str(), "mgmt_http_SUITE");
}

#[test]
fn app_covered_by_same_app_caller_is_not_uncovered() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "deps/rabbit/src/rabbit_db.erl",
        "-module(rabbit_db).\n",
    );
    touch(
        root,
        "deps/rabbit/test/db_SUITE.erl",
        "-module(db_SUITE).\ngo() -> rabbit_db:get(k).\n",
    );
    let apps = vec![app("rabbit", root, vec!["rabbit_db"])];
    let p = plan(&input(root, vec!["deps/rabbit/src/rabbit_db.erl"], apps));
    assert!(!p.entries.is_empty());
    assert!(p.uncovered.is_empty());
}

// A library exercised only by another application's suite is not uncovered: coverage is reason-based.
#[test]
fn library_app_covered_via_cross_app_caller_is_not_uncovered() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "deps/rabbit_common/src/rabbit_misc.erl",
        "-module(rabbit_misc).\n",
    );
    touch(
        root,
        "deps/rabbit/test/misc_SUITE.erl",
        "-module(misc_SUITE).\ngo() -> rabbit_misc:format(x).\n",
    );
    let apps = vec![
        app("rabbit_common", root, vec!["rabbit_misc"]),
        app("rabbit", root, vec![]),
    ];
    let mut inp = input(root, vec!["deps/rabbit_common/src/rabbit_misc.erl"], apps);
    inp.library_apps = vec![ApplicationName::new("rabbit_common").unwrap()];
    let p = plan(&inp);
    assert!(!p.entries.is_empty());
    assert!(p.uncovered.is_empty());
}

// "This plugin has no tests" is signal, not a skip condition.
#[test]
fn app_with_no_suites_is_uncovered_with_empty_candidates() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "deps/tiny/src/tiny.erl", "-module(tiny).\n");
    let apps = vec![app("tiny", root, vec!["tiny"])];
    let p = plan(&input(root, vec!["deps/tiny/src/tiny.erl"], apps));
    assert_eq!(p.uncovered.len(), 1);
    assert_eq!(p.uncovered[0].discovered_suites, 0);
    assert!(p.uncovered[0].suites.is_empty());
}

// priv-only changes carry no modified modules: the advisory reports nothing.
#[test]
fn priv_only_change_does_not_fire_the_advisory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "deps/mgmt/priv/www/js/main.js", "x\n");
    touch(
        root,
        "deps/mgmt/test/mgmt_http_SUITE.erl",
        "-module(mgmt_http_SUITE).\n",
    );
    let apps = vec![app("mgmt", root, vec![])];
    let p = plan(&input(root, vec!["deps/mgmt/priv/www/js/main.js"], apps));
    assert!(p.uncovered.is_empty());
}

#[test]
fn paths_outside_any_application_are_counted_unattributed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "rabbitmq-components.mk", "dep_cowboy = hex 2.16.0\n");
    let apps = vec![app("rabbit", root, vec![])];
    let p = plan(&input(root, vec!["rabbitmq-components.mk"], apps));
    assert_eq!(p.unattributed_paths, 1);
    assert!(p.uncovered.is_empty());
}

// A configured rule firing inside the app marks it covered even when no suite references a modified module.
#[test]
fn configured_rule_coverage_suppresses_the_advisory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "deps/mgmt/src/mgmt_util.erl", "-module(mgmt_util).\n");
    touch(
        root,
        "deps/mgmt/test/mgmt_http_SUITE.erl",
        "-module(mgmt_http_SUITE).\nall() -> [].\n",
    );
    let apps = vec![app("mgmt", root, vec!["mgmt_util"])];
    let mut inp = input(root, vec!["deps/mgmt/src/mgmt_util.erl"], apps);
    inp.extra_rules = vec![ExtraRule {
        name: "management-http".to_owned(),
        trigger: ExtraRuleTrigger::PathContains {
            fragment: "deps/mgmt/".to_owned(),
        },
        include_suites: vec![SuiteRefSpec {
            application: ApplicationName::new("mgmt").unwrap(),
            module: ModuleName::new("mgmt_http_SUITE").unwrap(),
        }],
        include_suite_templates: Vec::new(),
        line_match: None,
        include_suite_for_dep_modules: false,
    }];
    let p = plan(&inp);
    assert_eq!(p.entries.len(), 1);
    assert!(p.uncovered.is_empty());
}

// Only the suite file changed: no modified source modules, nothing to report.
#[test]
fn suite_only_change_does_not_fire_the_advisory() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "deps/rabbit/test/db_SUITE.erl",
        "-module(db_SUITE).\n",
    );
    let apps = vec![app("rabbit", root, vec![])];
    let p = plan(&input(root, vec!["deps/rabbit/test/db_SUITE.erl"], apps));
    assert!(p.uncovered.is_empty());
}

// Two modified modules in one app collapse into one advisory row.
#[test]
fn multiple_modified_modules_report_one_row_per_app() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(root, "deps/mgmt/src/a.erl", "-module(a).\n");
    touch(root, "deps/mgmt/src/b.erl", "-module(b).\n");
    let apps = vec![app("mgmt", root, vec!["a", "b"])];
    let p = plan(&input(
        root,
        vec!["deps/mgmt/src/a.erl", "deps/mgmt/src/b.erl"],
        apps,
    ));
    assert_eq!(p.uncovered.len(), 1);
    assert_eq!(p.uncovered[0].modified_modules, 2);
}
