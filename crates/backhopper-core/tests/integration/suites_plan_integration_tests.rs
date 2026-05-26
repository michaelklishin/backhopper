// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::app_src::{AppSrcSpec, discover};
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::{
    ExtraRule, ExtraRuleTrigger, PlanInput, SuiteInclusionReason, SuiteRefSpec, plan,
};

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    apps: Vec<AppSrcSpec>,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path().to_path_buf();
    let rabbit = root.join("deps/rabbit");
    fs::create_dir_all(rabbit.join("src")).unwrap();
    fs::create_dir_all(rabbit.join("test")).unwrap();
    fs::write(
        rabbit.join("src/rabbit.app.src"),
        "{application, rabbit, [{vsn, \"4\"}, {modules, [rabbit_amqqueue, rabbit_db]}, {applications, [kernel, stdlib]}]}.",
    )
    .unwrap();
    fs::write(
        rabbit.join("src/rabbit_amqqueue.erl"),
        "-module(rabbit_amqqueue).",
    )
    .unwrap();
    fs::write(rabbit.join("src/rabbit_db.erl"), "-module(rabbit_db).").unwrap();
    fs::write(
        rabbit.join("test/vhost_SUITE.erl"),
        "-module(vhost_SUITE).\ngo() -> rabbit_db:list().",
    )
    .unwrap();
    fs::write(
        rabbit.join("test/unrelated_SUITE.erl"),
        "-module(unrelated_SUITE).\ngo() -> ok.",
    )
    .unwrap();
    fs::write(
        rabbit.join("test/unit_helpers_SUITE.erl"),
        "-module(unit_helpers_SUITE).\ngo() -> rabbit_amqqueue:start().",
    )
    .unwrap();

    let mgmt = root.join("deps/rabbitmq_management");
    fs::create_dir_all(mgmt.join("src")).unwrap();
    fs::create_dir_all(mgmt.join("test")).unwrap();
    fs::write(
        mgmt.join("src/rabbitmq_management.app.src"),
        "{application, rabbitmq_management, [{vsn, \"4\"}, {modules, []}, {applications, [rabbit]}]}.",
    )
    .unwrap();
    fs::write(
        mgmt.join("test/connection_SUITE.erl"),
        "-module(connection_SUITE).\ngo() -> rabbit_amqqueue:declare().",
    )
    .unwrap();

    let apps = discover(&root).specs;
    Fixture {
        _dir: dir,
        root,
        apps,
    }
}

#[test]
fn r1_test_modified_includes_directly_edited_suite() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/test/vhost_SUITE.erl")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    assert_eq!(result.len(), 1);
    let entry = &result.entries[0];
    assert_eq!(entry.suite.module.as_str(), "vhost_SUITE");
    assert!(matches!(
        entry.reasons[0],
        SuiteInclusionReason::TestModified { .. }
    ));
}

#[test]
fn r4_same_app_caller_includes_grep_positive_suites() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/src/rabbit_db.erl")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    let suite_names: Vec<&str> = result
        .entries
        .iter()
        .map(|e| e.suite.module.as_str())
        .collect();
    assert!(
        suite_names.contains(&"vhost_SUITE"),
        "vhost_SUITE references rabbit_db, should be included: {:?}",
        suite_names
    );
    assert!(!suite_names.contains(&"unrelated_SUITE"));
}

#[test]
fn r5_cross_app_caller_only_fires_for_library_modules() {
    let f = fixture();
    let library_apps = vec![ApplicationName::new("rabbit").unwrap()];
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/src/rabbit_amqqueue.erl")],
        apps: f.apps.clone(),
        library_apps,
        extra_rules: vec![],
    };
    let result = plan(&input);
    let mgmt_suites: Vec<&str> = result
        .entries
        .iter()
        .filter(|e| e.suite.application.as_str() == "rabbitmq_management")
        .map(|e| e.suite.module.as_str())
        .collect();
    assert!(
        mgmt_suites.contains(&"connection_SUITE"),
        "connection_SUITE in rabbitmq_management references rabbit_amqqueue: {:?}",
        mgmt_suites
    );
    let reasons = result.entries.iter().find_map(|e| {
        if e.suite.module.as_str() == "connection_SUITE" {
            Some(&e.reasons)
        } else {
            None
        }
    });
    assert!(
        reasons
            .unwrap()
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::CrossAppCaller { .. }))
    );
}

#[test]
fn r5_does_not_fire_when_module_app_is_not_library() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/src/rabbit_amqqueue.erl")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    let mgmt_suites: Vec<&str> = result
        .entries
        .iter()
        .filter(|e| e.suite.application.as_str() == "rabbitmq_management")
        .map(|e| e.suite.module.as_str())
        .collect();
    assert!(
        mgmt_suites.is_empty(),
        "no cross-app suites when library_apps is empty, got {:?}",
        mgmt_suites
    );
}

#[test]
fn r3_unit_or_prop_sweep_includes_matching_suites_with_module_refs() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/src/rabbit_amqqueue.erl")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    let reasons: Vec<&SuiteInclusionReason> = result
        .entries
        .iter()
        .find(|e| e.suite.module.as_str() == "unit_helpers_SUITE")
        .map(|e| e.reasons.iter().collect())
        .unwrap_or_default();
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::UnitOrPropSweep { .. })),
        "unit_helpers_SUITE should be picked by R3, reasons={:?}",
        reasons
    );
}

#[test]
fn multiple_reasons_accumulate_on_the_same_suite() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![
            PathBuf::from("deps/rabbit/test/vhost_SUITE.erl"),
            PathBuf::from("deps/rabbit/src/rabbit_db.erl"),
        ],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    let reasons: Vec<&SuiteInclusionReason> = result
        .entries
        .iter()
        .find(|e| e.suite.module.as_str() == "vhost_SUITE")
        .map(|e| e.reasons.iter().collect())
        .unwrap_or_default();
    assert!(reasons.len() >= 2);
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::TestModified { .. }))
    );
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::SameAppCaller { .. }))
    );
}

#[test]
fn configured_rule_path_suffix_includes_listed_suites() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/rabbit/priv/schema/rabbit.schema")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![ExtraRule {
            name: "schema-change".into(),
            trigger: ExtraRuleTrigger::PathSuffix {
                suffix: ".schema".into(),
            },
            include_suites: vec![SuiteRefSpec {
                application: ApplicationName::new("rabbit").unwrap(),
                module: ModuleName::new("unrelated_SUITE").unwrap(),
            }],
            include_suite_templates: Vec::new(),
        }],
    };
    let result = plan(&input);
    let reasons: Vec<&SuiteInclusionReason> = result
        .entries
        .iter()
        .find(|e| e.suite.module.as_str() == "unrelated_SUITE")
        .map(|e| e.reasons.iter().collect())
        .unwrap_or_default();
    assert!(
        reasons.iter().any(|r| matches!(
            r,
            SuiteInclusionReason::ConfiguredRule { rule_name, .. } if rule_name == "schema-change"
        )),
        "configured rule should have included unrelated_SUITE, reasons={:?}",
        reasons
    );
}

#[test]
fn empty_diff_yields_empty_plan() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![],
    };
    let result = plan(&input);
    assert!(result.is_empty());
}
