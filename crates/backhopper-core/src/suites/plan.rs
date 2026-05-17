//! Top-level `plan` entry point: composes built-in rules and any
//! configured rules into one ordered output.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::suites::model::{PlanInput, SuiteInclusionReason, SuitePlan, SuitePlanEntry, SuiteRef};
use crate::suites::{rules, scan};

pub fn plan(input: &PlanInput) -> SuitePlan {
    let discovered = scan::enumerate_suites(&input.repo_root, &input.apps);
    let classification = rules::classify(&input.modified_paths, &input.apps, &input.repo_root);
    let mut suite_text_cache: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut accum: BTreeMap<SuiteRef, Vec<SuiteInclusionReason>> = BTreeMap::new();
    rules::apply_test_modified(&classification, &discovered, &mut accum);
    rules::apply_same_app_caller(
        &classification,
        &discovered,
        &mut suite_text_cache,
        &mut accum,
    );
    rules::apply_cross_app_caller(
        &classification,
        &discovered,
        &input.library_apps,
        &mut suite_text_cache,
        &mut accum,
    );
    rules::apply_unit_or_prop_sweep(
        &classification,
        &discovered,
        &mut suite_text_cache,
        &mut accum,
    );
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
