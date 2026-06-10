// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Top-level `plan` entry point: composes built-in rules and any
//! configured rules into one ordered output.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::app_src::AppSrcSpec;
use crate::model::names::{ApplicationName, ModuleName};
use crate::suites::matcher::{SubstringMatcher, SuiteMatcher};
use crate::suites::model::{
    PlanInput, SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef, UncoveredApplication,
};
use crate::suites::rules::ModifiedClassification;
use crate::suites::{rules, scan};

/// Plans against the default substring matcher. Use
/// `plan_with_matcher` to inject an AST-aware strategy.
pub fn plan(input: &PlanInput) -> SuitePlan {
    let mut matcher = SubstringMatcher::new();
    plan_with_matcher(input, &mut matcher)
}

pub fn plan_with_matcher(input: &PlanInput, matcher: &mut dyn SuiteMatcher) -> SuitePlan {
    let discovered = scan::enumerate_suites(&input.repo_root, &input.apps);
    let classification = rules::classify(&input.modified_paths, &input.apps, &input.repo_root);
    let mut accum: BTreeMap<SuiteRef, Vec<SuiteInclusionReason>> = BTreeMap::new();
    rules::apply_test_modified(&classification, &discovered, &mut accum);
    rules::apply_same_app_caller(&classification, &discovered, matcher, &mut accum);
    rules::apply_cross_app_caller(
        &classification,
        &discovered,
        &input.library_apps,
        matcher,
        &mut accum,
    );
    rules::apply_unit_or_prop_sweep(&classification, &discovered, matcher, &mut accum);
    rules::apply_behaviour_implementer_sweep(
        &classification,
        &discovered,
        &input.implementer_index,
        matcher,
        &mut accum,
    );
    rules::apply_configured_rules(
        &input.extra_rules,
        &input.modified_paths,
        &discovered,
        &input.repo_root,
        &input.dep_module_index,
        matcher,
        &mut accum,
    );
    let uncovered = uncovered_applications(
        &classification,
        &discovered,
        &accum,
        &input.apps,
        &input.repo_root,
    );
    let unattributed_paths = input
        .modified_paths
        .iter()
        .filter(|p| scan::application_of_path(&input.apps, &input.repo_root, p).is_none())
        .count();
    let entries = accum
        .into_iter()
        .map(|(suite, reasons)| SuitePlanEntry { suite, reasons })
        .collect();
    SuitePlan {
        entries,
        uncovered,
        unattributed_paths,
    }
}

/// Applications with modified source modules that no plan entry
/// covers. Coverage is reason-based, not entry-ownership-based: a
/// library application whose modules are exercised by another
/// application's suite (`CrossAppCaller`) counts as covered.
fn uncovered_applications(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    accum: &BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
    apps: &[AppSrcSpec],
    repo_root: &Path,
) -> Vec<UncoveredApplication> {
    let mut modified_by_app: BTreeMap<ApplicationName, BTreeSet<ModuleName>> = BTreeMap::new();
    for m in &classification.modified_modules {
        if let Some(app) = &m.application {
            modified_by_app
                .entry(app.clone())
                .or_default()
                .insert(m.module.clone());
        }
    }
    if modified_by_app.is_empty() {
        return Vec::new();
    }
    let mut covered_modules: BTreeSet<&ModuleName> = BTreeSet::new();
    let mut covered_apps: BTreeSet<&ApplicationName> = BTreeSet::new();
    for reasons in accum.values() {
        for reason in reasons {
            match reason {
                SuiteInclusionReason::SameAppCaller { module, .. }
                | SuiteInclusionReason::CrossAppCaller { module, .. } => {
                    covered_modules.insert(module);
                }
                SuiteInclusionReason::UnitOrPropSweep {
                    triggering_modules, ..
                } => {
                    covered_modules.extend(triggering_modules.iter());
                }
                SuiteInclusionReason::BehaviourImplementerSweep { behaviour, .. } => {
                    covered_modules.insert(behaviour);
                }
                SuiteInclusionReason::TestModified { path }
                | SuiteInclusionReason::ConfiguredRule {
                    triggering_path: path,
                    ..
                } => {
                    if let Some(app) = scan::application_of_path(apps, repo_root, path) {
                        covered_apps.insert(app);
                    }
                }
            }
        }
    }
    modified_by_app
        .into_iter()
        .filter(|(app, modules)| {
            !covered_apps.contains(app) && !modules.iter().any(|m| covered_modules.contains(m))
        })
        .map(|(application, modules)| {
            let suites: Vec<ModuleName> = discovered
                .iter()
                .filter(|s| s.application == application)
                .map(|s| s.module.clone())
                .collect();
            UncoveredApplication {
                application,
                modified_modules: modules.len(),
                discovered_suites: suites.len(),
                suites,
            }
        })
        .collect()
}
