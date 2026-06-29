// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The per-module suite-fanout cap (042 §A, §B, §C): a module that
//! reaches an outsized share of suites is escalated to a
//! `BroadImpactModule` row and the suites it alone pulled in are dropped,
//! while suites kept by an independent reason survive.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use backhopper_core::app_src::AppSrcSpec;
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::{
    ExtraRule, ExtraRuleTrigger, PlanInput, SuiteInclusionReason, SuitePlan, SuiteRefSpec, plan,
};

const HELPER: &str = "rabbit_ct_broker_helpers";
const HELPER2: &str = "rabbit_ct_helpers";
const HELPER_APP: &str = "rabbitmq_ct_helpers";
const NARROW: &str = "rabbit_amqqueue";
const APP: &str = "rabbit";

fn touch(dir: &Path, rel: &str, body: &str) -> PathBuf {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, body).unwrap();
    full
}

fn app(name: &str, root: &Path, modules: Vec<&str>) -> AppSrcSpec {
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

// Writes `count` suites in `APP`'s test dir, each named `{prefix}{i}_SUITE`
// and calling every module in `refs`. Names avoid the unit/prop pattern so
// only the caller rules fire.
fn write_suites(root: &Path, prefix: &str, count: usize, refs: &[&str]) {
    for i in 0..count {
        let name = format!("{prefix}{i}_SUITE");
        let mut calls = String::new();
        for m in refs {
            calls.push_str(m);
            calls.push_str(":do(), ");
        }
        let body = format!("-module({name}).\ngo() -> {calls}ok.\n");
        touch(root, &format!("deps/{APP}/test/{name}.erl"), &body);
    }
}

fn input(root: &Path, modified: Vec<&str>, apps: Vec<AppSrcSpec>, library: Vec<&str>) -> PlanInput {
    PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: modified.into_iter().map(PathBuf::from).collect(),
        apps,
        library_apps: library
            .into_iter()
            .map(|a| ApplicationName::new(a).unwrap())
            .collect(),
        extra_rules: Vec::new(),
        implementer_index: BTreeMap::new(),
        dep_module_index: BTreeMap::new(),
    }
}

// The standard two-application tree: a library `HELPER_APP` holding the
// helper modules, and `APP` holding the narrow module and every suite.
// `helper_src` and `narrow_src` decide which source files are written and
// returned as modified paths.
fn standard_apps(root: &Path) -> Vec<AppSrcSpec> {
    touch(
        root,
        &format!("deps/{HELPER_APP}/src/{HELPER}.erl"),
        &format!("-module({HELPER}).\n"),
    );
    touch(
        root,
        &format!("deps/{HELPER_APP}/src/{HELPER2}.erl"),
        &format!("-module({HELPER2}).\n"),
    );
    touch(
        root,
        &format!("deps/{APP}/src/{NARROW}.erl"),
        &format!("-module({NARROW}).\n"),
    );
    vec![
        app(HELPER_APP, root, vec![HELPER, HELPER2]),
        app(APP, root, vec![NARROW]),
    ]
}

const HELPER_SRC: &str = "deps/rabbitmq_ct_helpers/src/rabbit_ct_broker_helpers.erl";
const HELPER2_SRC: &str = "deps/rabbitmq_ct_helpers/src/rabbit_ct_helpers.erl";
const NARROW_SRC: &str = "deps/rabbit/src/rabbit_amqqueue.erl";

fn modules(p: &SuitePlan) -> Vec<&str> {
    p.entries.iter().map(|e| e.suite.module.as_str()).collect()
}

// A helper referenced by ten of twelve suites is escalated; the two
// suites driven by the narrow module survive.
#[test]
fn broad_module_escalates_and_narrow_suites_survive() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    write_suites(root, "nq", 2, &[NARROW]);
    let p = plan(&input(
        root,
        vec![HELPER_SRC, NARROW_SRC],
        apps,
        vec![HELPER_APP],
    ));

    assert_eq!(p.entries.len(), 2);
    assert_eq!(modules(&p), vec!["nq0_SUITE", "nq1_SUITE"]);
    assert_eq!(p.broad_impact.len(), 1);
    let b = &p.broad_impact[0];
    assert_eq!(b.module.as_str(), HELPER);
    assert_eq!(b.application.as_ref().unwrap().as_str(), HELPER_APP);
    assert_eq!(b.suite_fanout, 10);
    assert_eq!(b.total_suites, 12);
}

// A suite reached by both the broad helper and the narrow module stays,
// attributed to the narrow module only after the broad reason is dropped.
#[test]
fn dual_reason_suite_survives_attributed_to_narrow() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    write_suites(root, "du", 1, &[HELPER, NARROW]);
    let p = plan(&input(
        root,
        vec![HELPER_SRC, NARROW_SRC],
        apps,
        vec![HELPER_APP],
    ));

    assert_eq!(modules(&p), vec!["du0_SUITE"]);
    let reasons = &p.entries[0].reasons;
    assert_eq!(reasons.len(), 1);
    assert!(matches!(
        &reasons[0],
        SuiteInclusionReason::SameAppCaller { module, .. } if module.as_str() == NARROW
    ));
    assert_eq!(p.broad_impact[0].suite_fanout, 11);
}

// Only the helper changed: every suite is helper-only, so the plan is an
// explicit broad verdict with no entries.
#[test]
fn all_broad_yields_empty_entries() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));

    assert!(p.entries.is_empty());
    assert_eq!(p.broad_impact.len(), 1);
    assert_eq!(p.broad_impact[0].suite_fanout, 10);
    assert_eq!(p.broad_impact[0].total_suites, 10);
}

// The cap runs after `uncovered_applications` reads the full accumulator,
// so a broad module still counts as covering its application and is never
// double-reported as uncovered.
#[test]
fn broad_module_is_not_double_reported_as_uncovered() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));

    assert!(p.entries.is_empty());
    assert_eq!(p.broad_impact.len(), 1);
    assert!(p.uncovered.is_empty());
}

// A seven-suite reach is under `FANOUT_FLOOR`, so the helper is not broad
// and every referencing suite is kept.
#[test]
fn fanout_below_the_floor_is_not_broad() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 7, &[HELPER]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));

    assert_eq!(p.entries.len(), 7);
    assert!(p.broad_impact.is_empty());
}

// At exactly `FANOUT_FLOOR` reaches, with a relative share over a third,
// the helper is broad.
#[test]
fn fanout_at_the_floor_is_broad() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 8, &[HELPER]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));

    assert!(p.entries.is_empty());
    assert_eq!(p.broad_impact.len(), 1);
    assert_eq!(p.broad_impact[0].suite_fanout, 8);
}

// A `TestModified` suite is path-driven (it attributes to no module), so
// the non-empty guard keeps it even when its only module reference is the
// broad helper.
#[test]
fn test_modified_suite_survives_broad_helper() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    let modified_suite = "deps/rabbit/test/br0_SUITE.erl";
    let p = plan(&input(
        root,
        vec![HELPER_SRC, modified_suite],
        apps,
        vec![HELPER_APP],
    ));

    assert_eq!(modules(&p), vec!["br0_SUITE"]);
    let reasons = &p.entries[0].reasons;
    assert!(
        reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::TestModified { .. }))
    );
    assert!(
        !reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::CrossAppCaller { .. }))
    );
    assert_eq!(p.broad_impact.len(), 1);
}

// A `ConfiguredRule` suite is likewise path-driven and survives the cap.
#[test]
fn configured_rule_suite_survives_broad_helper() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER]);
    let mut inp = input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]);
    inp.extra_rules = vec![ExtraRule {
        name: "broker-helpers".to_owned(),
        trigger: ExtraRuleTrigger::PathContains {
            fragment: format!("deps/{HELPER_APP}/"),
        },
        include_suites: vec![SuiteRefSpec {
            application: ApplicationName::new(APP).unwrap(),
            module: ModuleName::new("br0_SUITE").unwrap(),
        }],
        include_suite_templates: Vec::new(),
        line_match: None,
        include_suite_for_dep_modules: false,
    }];
    let p = plan(&inp);

    assert_eq!(modules(&p), vec!["br0_SUITE"]);
    assert!(
        p.entries[0]
            .reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::ConfiguredRule { .. }))
    );
    assert_eq!(p.broad_impact.len(), 1);
}

// The fraction test is strict: `suite_fanout * FANOUT_DEN > total_suites`.
// At equality the helper is not broad; one suite fewer in the tree tips it
// over.
#[test]
fn threshold_is_strict_greater_than() {
    // Equality: fanout 8, total 24, 8 * 3 == 24, not broad.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 8, &[HELPER]);
    write_suites(root, "fl", 16, &[]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));
    assert_eq!(p.entries.len(), 8);
    assert!(p.broad_impact.is_empty());

    // One above: fanout 8, total 23, 24 > 23, broad.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 8, &[HELPER]);
    write_suites(root, "fl", 15, &[]);
    let p = plan(&input(root, vec![HELPER_SRC], apps, vec![HELPER_APP]));
    assert!(p.entries.is_empty());
    assert_eq!(p.broad_impact.len(), 1);
    assert_eq!(p.broad_impact[0].total_suites, 23);
}

// Two helpers both over the cap yield two rows in module-sorted order, so
// the report is stable.
#[test]
fn multiple_broad_modules_are_sorted() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let apps = standard_apps(root);
    write_suites(root, "br", 10, &[HELPER, HELPER2]);
    let p = plan(&input(
        root,
        vec![HELPER_SRC, HELPER2_SRC],
        apps,
        vec![HELPER_APP],
    ));

    assert!(p.entries.is_empty());
    let names: Vec<&str> = p.broad_impact.iter().map(|b| b.module.as_str()).collect();
    assert_eq!(names, vec![HELPER, HELPER2]);
    assert!(p.broad_impact.iter().all(|b| b.suite_fanout == 10));
}

// A broad helper whose every referencing suite also has an independent
// narrow reason removes no suite from the plan: the broad reason is
// stripped, each suite survives on its narrow reason, and the broad row is
// still reported as the fanout signal.
#[test]
fn broad_helper_strips_its_reason_but_keeps_dual_suites() {
    let narrow_mods = [
        "rabbit_amqqueue",
        "rabbit_exchange",
        "rabbit_binding",
        "rabbit_vhost",
        "rabbit_queue_index",
        "rabbit_channel",
        "rabbit_connection",
        "rabbit_policy",
    ];
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        &format!("deps/{HELPER_APP}/src/{HELPER}.erl"),
        &format!("-module({HELPER}).\n"),
    );
    let mut modified = vec![HELPER_SRC.to_owned()];
    for (i, m) in narrow_mods.iter().enumerate() {
        touch(
            root,
            &format!("deps/{APP}/src/{m}.erl"),
            &format!("-module({m}).\n"),
        );
        modified.push(format!("deps/{APP}/src/{m}.erl"));
        let name = format!("dual{i}_SUITE");
        touch(
            root,
            &format!("deps/{APP}/test/{name}.erl"),
            &format!("-module({name}).\ngo() -> {HELPER}:do(), {m}:do(), ok.\n"),
        );
    }
    let apps = vec![
        app(HELPER_APP, root, vec![HELPER]),
        app(APP, root, narrow_mods.to_vec()),
    ];
    let modified: Vec<&str> = modified.iter().map(String::as_str).collect();
    let p = plan(&input(root, modified, apps, vec![HELPER_APP]));

    assert_eq!(p.entries.len(), 8);
    assert_eq!(p.broad_impact.len(), 1);
    assert_eq!(p.broad_impact[0].module.as_str(), HELPER);
    assert_eq!(p.broad_impact[0].suite_fanout, 8);
    for e in &p.entries {
        assert!(
            e.reasons
                .iter()
                .all(|r| !matches!(r, SuiteInclusionReason::CrossAppCaller { .. }))
        );
        assert!(
            e.reasons
                .iter()
                .any(|r| matches!(r, SuiteInclusionReason::SameAppCaller { .. }))
        );
    }
}
