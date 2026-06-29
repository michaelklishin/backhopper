// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants of the suite-fanout cap (042): below the floor the cap is a
//! no-op, and the output never depends on modified-path input order.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use tempfile::TempDir;

use backhopper_core::app_src::AppSrcSpec;
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::{PlanInput, plan};

const HELPER: &str = "rabbit_ct_broker_helpers";
const HELPER_APP: &str = "rabbitmq_ct_helpers";
const NARROW: &str = "rabbit_amqqueue";
const APP: &str = "rabbit";
const HELPER_SRC: &str = "deps/rabbitmq_ct_helpers/src/rabbit_ct_broker_helpers.erl";
const NARROW_SRC: &str = "deps/rabbit/src/rabbit_amqqueue.erl";

fn touch(dir: &Path, rel: &str, body: &str) {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, body).unwrap();
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

// Builds the two-application tree and returns the apps. The helper and
// narrow source files double as the modified paths the tests pass in.
fn build(root: &Path, n_helper: usize, n_narrow: usize, n_filler: usize) -> Vec<AppSrcSpec> {
    touch(root, HELPER_SRC, &format!("-module({HELPER}).\n"));
    touch(root, NARROW_SRC, &format!("-module({NARROW}).\n"));
    write_suites(root, "br", n_helper, &[HELPER]);
    write_suites(root, "nq", n_narrow, &[NARROW]);
    write_suites(root, "fl", n_filler, &[]);
    vec![
        app(HELPER_APP, root, vec![HELPER]),
        app(APP, root, vec![NARROW]),
    ]
}

fn input(root: &Path, modified: Vec<&str>, apps: Vec<AppSrcSpec>) -> PlanInput {
    PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: modified.into_iter().map(PathBuf::from).collect(),
        apps,
        library_apps: vec![ApplicationName::new(HELPER_APP).unwrap()],
        extra_rules: Vec::new(),
        implementer_index: BTreeMap::new(),
        dep_module_index: BTreeMap::new(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // Below `FANOUT_FLOOR` no module is broad, so the cap drops nothing:
    // `broad_impact` is empty and every suite a rule selected survives.
    // This bounds the blast radius to the broad case.
    #[test]
    fn below_the_floor_is_a_no_op(
        n_helper in 0usize..=7,
        n_narrow in 0usize..=5,
        n_filler in 0usize..=5,
    ) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let apps = build(root, n_helper, n_narrow, n_filler);
        let p = plan(&input(root, vec![HELPER_SRC, NARROW_SRC], apps));
        prop_assert!(p.broad_impact.is_empty());
        prop_assert_eq!(p.entries.len(), n_helper + n_narrow);
    }

    // The broad classification and the surviving entry set do not depend
    // on the order of modified paths.
    #[test]
    fn output_is_independent_of_modified_path_order(
        n_helper in 0usize..=12,
        n_narrow in 0usize..=4,
    ) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let apps = build(root, n_helper, n_narrow, 0);
        let forward = plan(&input(root, vec![HELPER_SRC, NARROW_SRC], apps.clone()));
        let reversed = plan(&input(root, vec![NARROW_SRC, HELPER_SRC], apps));
        prop_assert_eq!(&forward.entries, &reversed.entries);
        prop_assert_eq!(&forward.broad_impact, &reversed.broad_impact);
    }
}
