//! Built-in suite-selection rules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::slice;

use crate::app_src::AppSrcSpec;
use crate::model::names::{ApplicationName, ModuleName};
use crate::suites::matcher::SuiteMatcher;
use crate::suites::model::{ExtraRule, ExtraRuleTrigger, SuiteInclusionReason, SuiteRef};
use crate::suites::scan;

/// A modified module: the file path that was changed, plus the
/// module name (basename minus `.erl`) and the application it lives
/// in (when resolvable).
#[derive(Debug, Clone)]
pub(crate) struct ModifiedModule {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub module: ModuleName,
    pub application: Option<ApplicationName>,
}

/// Classify modified paths into test-files, source-modules, and
/// "other" buckets the built-in rules don't act on.
pub(crate) struct ModifiedClassification {
    pub test_paths: Vec<PathBuf>,
    pub modified_modules: Vec<ModifiedModule>,
}

pub(crate) fn classify(
    modified_paths: &[PathBuf],
    apps: &[AppSrcSpec],
    repo_root: &Path,
) -> ModifiedClassification {
    let mut test_paths = Vec::new();
    let mut modified_modules = Vec::new();
    for p in modified_paths {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with("_SUITE.erl") {
            test_paths.push(p.clone());
            continue;
        }
        if let Some(base) = name.strip_suffix(".erl")
            && is_under_src_or_include(p)
            && let Ok(module) = ModuleName::new(base)
        {
            let application = scan::application_of_path(apps, repo_root, p).cloned();
            modified_modules.push(ModifiedModule {
                path: p.clone(),
                module,
                application,
            });
        }
    }
    ModifiedClassification {
        test_paths,
        modified_modules,
    }
}

fn is_under_src_or_include(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str() == "src" || c.as_os_str() == "include")
}

/// TestModified (R1): if a `_SUITE.erl` appears in the diff, include
/// the matching suite from the discovered suite list.
pub(crate) fn apply_test_modified(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    for test_path in &classification.test_paths {
        let basename = test_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let module_name = basename.strip_suffix(".erl").unwrap_or(basename);
        let Some(suite) = discovered.iter().find(|s| s.module.as_str() == module_name) else {
            continue;
        };
        out.entry(suite.clone())
            .or_default()
            .push(SuiteInclusionReason::TestModified {
                path: test_path.clone(),
            });
    }
}

/// SameAppCaller (R4): each modified non-test module M; suites in
/// the same application that reference M get included.
pub(crate) fn apply_same_app_caller(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    for m in &classification.modified_modules {
        let Some(app) = &m.application else { continue };
        for suite in discovered.iter().filter(|s| &s.application == app) {
            let refs = matcher.modules_referenced_in_suite(&suite.path, slice::from_ref(&m.module));
            if refs.is_empty() {
                continue;
            }
            out.entry(suite.clone())
                .or_default()
                .push(SuiteInclusionReason::SameAppCaller {
                    application: app.clone(),
                    module: m.module.clone(),
                });
        }
    }
}

/// CrossAppCaller (R5): for each modified non-test module M whose
/// application is in `library_apps`, suites in *other* applications
/// that reference M get included.
pub(crate) fn apply_cross_app_caller(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    library_apps: &[ApplicationName],
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    let library_set: BTreeSet<&ApplicationName> = library_apps.iter().collect();
    for m in &classification.modified_modules {
        let Some(app) = &m.application else { continue };
        if !library_set.contains(app) {
            continue;
        }
        for suite in discovered.iter().filter(|s| &s.application != app) {
            let refs = matcher.modules_referenced_in_suite(&suite.path, slice::from_ref(&m.module));
            if refs.is_empty() {
                continue;
            }
            out.entry(suite.clone())
                .or_default()
                .push(SuiteInclusionReason::CrossAppCaller {
                    library_application: app.clone(),
                    module: m.module.clone(),
                });
        }
    }
}

/// UnitOrPropSweep (R3 ∩ R4 refinement): for each application that
/// has modified source, sweep suites whose name matches the
/// unit-or-prop pattern AND that reference at least one modified
/// module from the same application.
pub(crate) fn apply_unit_or_prop_sweep(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    let mut by_app: BTreeMap<ApplicationName, Vec<ModuleName>> = BTreeMap::new();
    for m in &classification.modified_modules {
        if let Some(app) = &m.application {
            by_app
                .entry(app.clone())
                .or_default()
                .push(m.module.clone());
        }
    }
    for (app, modules) in &by_app {
        for suite in discovered
            .iter()
            .filter(|s| &s.application == app)
            .filter(|s| is_unit_or_prop_suite(&s.module))
        {
            let refs = matcher.modules_referenced_in_suite(&suite.path, modules);
            if refs.is_empty() {
                continue;
            }
            out.entry(suite.clone())
                .or_default()
                .push(SuiteInclusionReason::UnitOrPropSweep {
                    application: app.clone(),
                    triggering_modules: refs.into_iter().collect(),
                });
        }
    }
}

/// True for suite names matching common fast-suite conventions:
/// `unit_*_SUITE`, `*_unit_*_SUITE`, `prop_*_SUITE`, `*_prop_*_SUITE`.
fn is_unit_or_prop_suite(module: &ModuleName) -> bool {
    let n = module.as_str();
    n.starts_with("unit_") || n.starts_with("prop_") || n.contains("_unit_") || n.contains("_prop_")
}

/// ConfiguredRule: apply each user-supplied rule. When its trigger
/// fires, include every suite listed in `include_suites`.
pub(crate) fn apply_configured_rules(
    rules: &[ExtraRule],
    modified_paths: &[PathBuf],
    discovered: &[SuiteRef],
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    for rule in rules {
        for path in modified_paths {
            if !trigger_fires(&rule.trigger, path) {
                continue;
            }
            for spec in &rule.include_suites {
                let Some(suite) = discovered
                    .iter()
                    .find(|s| s.application == spec.application && s.module == spec.module)
                else {
                    continue;
                };
                out.entry(suite.clone())
                    .or_default()
                    .push(SuiteInclusionReason::ConfiguredRule {
                        rule_name: rule.name.clone(),
                        triggering_path: path.clone(),
                    });
            }
        }
    }
}

fn trigger_fires(trigger: &ExtraRuleTrigger, path: &Path) -> bool {
    let s = path.to_string_lossy();
    match trigger {
        ExtraRuleTrigger::PathSuffix { suffix } => s.ends_with(suffix.as_str()),
        ExtraRuleTrigger::PathContains { fragment } => s.contains(fragment.as_str()),
    }
}
