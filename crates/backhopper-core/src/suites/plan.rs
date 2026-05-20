// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Top-level `plan` entry point: composes built-in rules and any
//! configured rules into one ordered output.

use std::collections::BTreeMap;

use crate::suites::matcher::{SubstringMatcher, SuiteMatcher};
use crate::suites::model::{PlanInput, SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef};
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
    rules::apply_configured_rules(
        &input.extra_rules,
        &input.modified_paths,
        &discovered,
        &mut accum,
    );
    let entries = accum
        .into_iter()
        .map(|(suite, reasons)| SuitePlanEntry { suite, reasons })
        .collect();
    SuitePlan { entries }
}