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
    BroadImpactModule, PlanInput, SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef,
    UncoveredApplication,
};
use crate::suites::rules::ModifiedClassification;
use crate::suites::{rules, scan};

/// A module is broad when it reaches at least `FANOUT_FLOOR` suites and
/// more than `FANOUT_NUM / FANOUT_DEN` of all discovered suites. The
/// floor stops a tiny tree from misfiring; the fraction scales with the
/// project. Defaults to calibrate against the corpus, not a fixed claim.
const FANOUT_FLOOR: usize = 8;
const FANOUT_NUM: usize = 1;
const FANOUT_DEN: usize = 3;

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

    // Fanout cap runs after `uncovered_applications` has read the full
    // accumulator, so a broad module still counts as covering its
    // application and the cap only reshapes `entries`.
    let (broad_modules, broad_impact) =
        broad_impact_modules(&classification, &accum, discovered.len());

    let entries = accum
        .into_iter()
        .filter_map(|(suite, reasons)| {
            let kept: Vec<SuiteInclusionReason> = reasons
                .into_iter()
                .filter(|reason| !is_broad_driven(reason, &broad_modules))
                .collect();
            (!kept.is_empty()).then_some(SuitePlanEntry {
                suite,
                reasons: kept,
            })
        })
        .collect();
    SuitePlan {
        entries,
        uncovered,
        unattributed_paths,
        broad_impact,
    }
}

/// Modules whose suite fanout is an outsized share of all discovered
/// suites, with the set used to drop their reasons. Read from the pre-cap
/// accumulator so a broad module still counts toward coverage in
/// `uncovered_applications`. A module's `application` is best-effort: a
/// reason can name a module whose application did not resolve.
fn broad_impact_modules(
    classification: &ModifiedClassification,
    accum: &BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
    total_suites: usize,
) -> (BTreeSet<ModuleName>, Vec<BroadImpactModule>) {
    let module_app: BTreeMap<&ModuleName, &ApplicationName> = classification
        .modified_modules
        .iter()
        .filter_map(|m| m.application.as_ref().map(|app| (&m.module, app)))
        .collect();
    let mut fanout: BTreeMap<&ModuleName, usize> = BTreeMap::new();
    for reasons in accum.values() {
        let mut attributed: BTreeSet<&ModuleName> = BTreeSet::new();
        for reason in reasons {
            attributed.extend(modules_of(reason));
        }
        for module in attributed {
            *fanout.entry(module).or_insert(0) += 1;
        }
    }
    let mut broad_modules = BTreeSet::new();
    let mut broad_impact = Vec::new();
    for (&module, &suite_fanout) in &fanout {
        if is_broad(suite_fanout, total_suites) {
            broad_modules.insert(module.clone());
            broad_impact.push(BroadImpactModule {
                module: module.clone(),
                application: module_app.get(module).map(|app| (*app).clone()),
                suite_fanout,
                total_suites,
            });
        }
    }
    (broad_modules, broad_impact)
}

/// Modules a single inclusion reason attributes to: the one definition
/// the fanout count and the survivor rule share. Wildcard-free so a new
/// reason variant must state its attributed modules (040 R12).
fn modules_of(reason: &SuiteInclusionReason) -> Vec<&ModuleName> {
    match reason {
        SuiteInclusionReason::SameAppCaller { module, .. }
        | SuiteInclusionReason::CrossAppCaller { module, .. } => vec![module],
        SuiteInclusionReason::UnitOrPropSweep {
            triggering_modules, ..
        } => triggering_modules.iter().collect(),
        SuiteInclusionReason::BehaviourImplementerSweep { behaviour, .. } => vec![behaviour],
        SuiteInclusionReason::TestModified { .. } | SuiteInclusionReason::ConfiguredRule { .. } => {
            Vec::new()
        }
    }
}

fn is_broad(suite_fanout: usize, total_suites: usize) -> bool {
    suite_fanout >= FANOUT_FLOOR && suite_fanout * FANOUT_DEN > total_suites * FANOUT_NUM
}

/// A reason is broad-driven when it attributes to at least one module and
/// every module it attributes to is broad. The non-empty guard is the
/// safety: a path-driven reason (`TestModified`, `ConfiguredRule`)
/// attributes to no module, so it is never broad-driven and always
/// survives.
fn is_broad_driven(reason: &SuiteInclusionReason, broad: &BTreeSet<ModuleName>) -> bool {
    let attributed = modules_of(reason);
    !attributed.is_empty() && attributed.iter().all(|module| broad.contains(*module))
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
