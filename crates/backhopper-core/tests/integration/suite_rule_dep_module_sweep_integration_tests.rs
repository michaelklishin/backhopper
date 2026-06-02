// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::app_src::{AppSrcSpec, discover};
use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::{
    ExtraRule, ExtraRuleTrigger, LineMatch, PlanInput, SuiteInclusionReason, plan,
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
        "{application, rabbit, [{vsn, \"4\"}, {modules, []}, {applications, [kernel, stdlib]}]}.",
    )
    .unwrap();
    fs::write(
        rabbit.join("test/config_schema_SUITE.erl"),
        "-module(config_schema_SUITE).\ngo() -> cuttlefish_generator:map(x).",
    )
    .unwrap();
    fs::write(
        rabbit.join("test/unrelated_SUITE.erl"),
        "-module(unrelated_SUITE).\ngo() -> ok.",
    )
    .unwrap();
    fs::write(
        root.join("rabbitmq-components.mk"),
        "dep_cuttlefish = hex 3.9.1\n",
    )
    .unwrap();

    let apps = discover(&root).specs;
    Fixture {
        _dir: dir,
        root,
        apps,
    }
}

fn cuttlefish_rule() -> ExtraRule {
    ExtraRule {
        name: "dep_bump_sweep".into(),
        trigger: ExtraRuleTrigger::PathRegex {
            pattern: "(^|/)rabbitmq-components\\.mk$".into(),
            captures: Vec::new(),
        },
        include_suites: Vec::new(),
        include_suite_templates: Vec::new(),
        line_match: Some(LineMatch {
            pattern: "^dep_(?P<dep>[a-z_]+)\\s*=\\s*hex\\s+".into(),
            captures: vec!["dep".to_owned()],
        }),
        include_suite_for_dep_modules: true,
    }
}

#[test]
fn dep_module_sweep_picks_suites_referencing_dep_modules() {
    let f = fixture();
    let mut dep_module_index: BTreeMap<String, Vec<ModuleName>> = BTreeMap::new();
    dep_module_index.insert(
        "cuttlefish".into(),
        vec![ModuleName::new("cuttlefish_generator").unwrap()],
    );
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("rabbitmq-components.mk")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![cuttlefish_rule()],
        implementer_index: BTreeMap::new(),
        dep_module_index,
    };
    let result = plan(&input);
    let names: Vec<&str> = result
        .entries
        .iter()
        .map(|e| e.suite.module.as_str())
        .collect();
    assert!(
        names.contains(&"config_schema_SUITE"),
        "expected config_schema_SUITE swept by dep_module_index, got {names:?}"
    );
    assert!(
        !names.contains(&"unrelated_SUITE"),
        "unrelated_SUITE references no cuttlefish module, got {names:?}"
    );
}

#[test]
fn dep_module_sweep_no_op_when_dep_index_empty() {
    let f = fixture();
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("rabbitmq-components.mk")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![cuttlefish_rule()],
        implementer_index: BTreeMap::new(),
        dep_module_index: BTreeMap::new(),
    };
    let result = plan(&input);
    assert!(
        result.is_empty(),
        "no rule fires with empty dep_module_index"
    );
}

#[test]
fn dep_module_sweep_works_with_dep_capture_from_path_only() {
    let f = fixture();
    let mut dep_module_index: BTreeMap<String, Vec<ModuleName>> = BTreeMap::new();
    dep_module_index.insert(
        "cuttlefish".into(),
        vec![ModuleName::new("cuttlefish_generator").unwrap()],
    );
    let rule = ExtraRule {
        name: "dep_change".into(),
        trigger: ExtraRuleTrigger::PathRegex {
            pattern: "deps/(?P<dep>[a-z_]+)/priv/schema/.*\\.schema$".into(),
            captures: vec!["dep".to_owned()],
        },
        include_suites: Vec::new(),
        include_suite_templates: Vec::new(),
        line_match: None,
        include_suite_for_dep_modules: true,
    };
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("deps/cuttlefish/priv/schema/foo.schema")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![rule],
        implementer_index: BTreeMap::new(),
        dep_module_index,
    };
    let result = plan(&input);
    let names: Vec<&str> = result
        .entries
        .iter()
        .map(|e| e.suite.module.as_str())
        .collect();
    assert!(names.contains(&"config_schema_SUITE"), "got {names:?}");
}

#[test]
fn dep_module_sweep_reason_carries_rule_name_and_path() {
    let f = fixture();
    let mut dep_module_index: BTreeMap<String, Vec<ModuleName>> = BTreeMap::new();
    dep_module_index.insert(
        "cuttlefish".into(),
        vec![ModuleName::new("cuttlefish_generator").unwrap()],
    );
    let input = PlanInput {
        repo_root: f.root.clone(),
        modified_paths: vec![PathBuf::from("rabbitmq-components.mk")],
        apps: f.apps.clone(),
        library_apps: vec![],
        extra_rules: vec![cuttlefish_rule()],
        implementer_index: BTreeMap::new(),
        dep_module_index,
    };
    let result = plan(&input);
    let entry = result
        .entries
        .iter()
        .find(|e| e.suite.module.as_str() == "config_schema_SUITE")
        .expect("config_schema_SUITE present");
    let SuiteInclusionReason::ConfiguredRule {
        rule_name,
        triggering_path,
    } = &entry.reasons[0]
    else {
        panic!("expected ConfiguredRule, got {:?}", entry.reasons[0]);
    };
    assert_eq!(rule_name, "dep_bump_sweep");
    assert_eq!(triggering_path, &PathBuf::from("rabbitmq-components.mk"));
}
