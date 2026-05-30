// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::app_src::AppSrcSpec;
use backhopper_core::model::names::{ApplicationName, ModuleName};
use backhopper_core::suites::{PlanInput, SuiteInclusionReason, plan};

fn touch(dir: &std::path::Path, rel: &str, body: &str) -> PathBuf {
    let full = dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, body).unwrap();
    full
}

fn app(name: &str, root: &std::path::Path, modules: Vec<&str>) -> AppSrcSpec {
    AppSrcSpec {
        name: ApplicationName::new(name).unwrap(),
        path: root.join(format!("apps/{name}/src/{name}.app.src")),
        vsn: None,
        modules: modules
            .into_iter()
            .map(|m| ModuleName::new(m).unwrap())
            .collect(),
        applications: BTreeSet::new(),
    }
}

#[test]
fn empty_index_does_not_emit_sweep_reasons() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "apps/rabbit/src/rabbit_backing_queue.erl",
        "-module(rabbit_backing_queue).\n",
    );
    touch(
        root,
        "apps/rabbit/test/foo_SUITE.erl",
        "-module(foo_SUITE).\nfoo() -> rabbit_variable_queue:bar().\n",
    );
    let apps = vec![app(
        "rabbit",
        root,
        vec!["rabbit_backing_queue", "rabbit_variable_queue"],
    )];
    let input = PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: vec![PathBuf::from("apps/rabbit/src/rabbit_backing_queue.erl")],
        apps,
        library_apps: Vec::new(),
        extra_rules: Vec::new(),
        implementer_index: BTreeMap::new(),
    };
    let p = plan(&input);
    let has_sweep = p.entries.iter().any(|e| {
        e.reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::BehaviourImplementerSweep { .. }))
    });
    assert!(!has_sweep);
}

#[test]
fn modified_behaviour_module_pulls_in_suites_referencing_implementer() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "apps/rabbit/src/rabbit_backing_queue.erl",
        "-module(rabbit_backing_queue).\n",
    );
    touch(
        root,
        "apps/rabbit/test/queue_behaviour_SUITE.erl",
        "-module(queue_behaviour_SUITE).\nfoo() -> rabbit_variable_queue:init().\n",
    );
    let apps = vec![app(
        "rabbit",
        root,
        vec!["rabbit_backing_queue", "rabbit_variable_queue"],
    )];
    let mut index: BTreeMap<ModuleName, Vec<ModuleName>> = BTreeMap::new();
    index.insert(
        ModuleName::new("rabbit_backing_queue").unwrap(),
        vec![ModuleName::new("rabbit_variable_queue").unwrap()],
    );
    let input = PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: vec![PathBuf::from("apps/rabbit/src/rabbit_backing_queue.erl")],
        apps,
        library_apps: Vec::new(),
        extra_rules: Vec::new(),
        implementer_index: index,
    };
    let p = plan(&input);
    let sweep = p.entries.iter().find_map(|e| {
        e.reasons.iter().find_map(|r| match r {
            SuiteInclusionReason::BehaviourImplementerSweep {
                behaviour,
                implementer,
            } => Some((behaviour.clone(), implementer.clone())),
            _ => None,
        })
    });
    assert_eq!(
        sweep,
        Some((
            ModuleName::new("rabbit_backing_queue").unwrap(),
            ModuleName::new("rabbit_variable_queue").unwrap(),
        ))
    );
}

#[test]
fn suite_without_implementer_reference_is_skipped() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    touch(
        root,
        "apps/rabbit/src/rabbit_backing_queue.erl",
        "-module(rabbit_backing_queue).\n",
    );
    touch(
        root,
        "apps/rabbit/test/unrelated_SUITE.erl",
        "-module(unrelated_SUITE).\nfoo() -> some_other:thing().\n",
    );
    let apps = vec![app(
        "rabbit",
        root,
        vec!["rabbit_backing_queue", "rabbit_variable_queue"],
    )];
    let mut index: BTreeMap<ModuleName, Vec<ModuleName>> = BTreeMap::new();
    index.insert(
        ModuleName::new("rabbit_backing_queue").unwrap(),
        vec![ModuleName::new("rabbit_variable_queue").unwrap()],
    );
    let input = PlanInput {
        repo_root: root.to_path_buf(),
        modified_paths: vec![PathBuf::from("apps/rabbit/src/rabbit_backing_queue.erl")],
        apps,
        library_apps: Vec::new(),
        extra_rules: Vec::new(),
        implementer_index: index,
    };
    let p = plan(&input);
    let has_sweep = p.entries.iter().any(|e| {
        e.reasons
            .iter()
            .any(|r| matches!(r, SuiteInclusionReason::BehaviourImplementerSweep { .. }))
    });
    assert!(!has_sweep, "got entries: {:?}", p.entries);
}
