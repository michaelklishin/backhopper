// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Built-in suite-selection rules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::slice;

use crate::app_src::AppSrcSpec;
use crate::model::names::{ApplicationName, ModuleName};
use std::fs;

use crate::suites::matcher::SuiteMatcher;
use crate::suites::model::{ExtraRule, ExtraRuleTrigger, SuiteInclusionReason, SuiteRef};
use crate::suites::scan;

/// A modified module: the file path that was changed, plus the
/// module name (basename minus `.erl`) and the application it belongs
/// to (when resolvable).
#[derive(Debug, Clone)]
pub(crate) struct ModifiedModule {
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
    // group by application so each suite's text is scanned once per app, not once per module
    let mut by_app: BTreeMap<&ApplicationName, Vec<ModuleName>> = BTreeMap::new();
    for m in &classification.modified_modules {
        if let Some(app) = &m.application {
            by_app.entry(app).or_default().push(m.module.clone());
        }
    }
    for (app, modules) in &by_app {
        for suite in discovered.iter().filter(|s| &s.application == *app) {
            let refs = matcher.modules_referenced_in_suite(&suite.path, modules);
            if refs.is_empty() {
                continue;
            }
            // Attribution stays per-module and in modified-module order.
            for m in &classification.modified_modules {
                if m.application.as_ref() == Some(*app) && refs.contains(&m.module) {
                    out.entry(suite.clone()).or_default().push(
                        SuiteInclusionReason::SameAppCaller {
                            application: (*app).clone(),
                            module: m.module.clone(),
                        },
                    );
                }
            }
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
    // group by owning application so each suite's text is scanned once per app, not once per module
    let mut by_app: BTreeMap<&ApplicationName, Vec<ModuleName>> = BTreeMap::new();
    for m in &classification.modified_modules {
        if let Some(app) = &m.application
            && library_set.contains(app)
        {
            by_app.entry(app).or_default().push(m.module.clone());
        }
    }
    for (app, modules) in &by_app {
        for suite in discovered.iter().filter(|s| &s.application != *app) {
            let refs = matcher.modules_referenced_in_suite(&suite.path, modules);
            if refs.is_empty() {
                continue;
            }
            for m in &classification.modified_modules {
                if m.application.as_ref() == Some(*app) && refs.contains(&m.module) {
                    out.entry(suite.clone()).or_default().push(
                        SuiteInclusionReason::CrossAppCaller {
                            library_application: (*app).clone(),
                            module: m.module.clone(),
                        },
                    );
                }
            }
        }
    }
}

/// Behaviour-implementer sweep: when a modified module is itself a
/// behaviour (some other module declares `-behaviour(M)`), every
/// implementer comes into scope and any suite that references one is
/// added. `implementer_index` is the behaviour-to-implementers map; an
/// empty map disables the rule.
pub(crate) fn apply_behaviour_implementer_sweep(
    classification: &ModifiedClassification,
    discovered: &[SuiteRef],
    implementer_index: &BTreeMap<ModuleName, Vec<ModuleName>>,
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    if implementer_index.is_empty() {
        return;
    }
    for m in &classification.modified_modules {
        let Some(implementers) = implementer_index.get(&m.module) else {
            continue;
        };
        for implementer in implementers {
            for suite in discovered.iter() {
                let refs =
                    matcher.modules_referenced_in_suite(&suite.path, slice::from_ref(implementer));
                if refs.is_empty() {
                    continue;
                }
                out.entry(suite.clone()).or_default().push(
                    SuiteInclusionReason::BehaviourImplementerSweep {
                        behaviour: m.module.clone(),
                        implementer: implementer.clone(),
                    },
                );
            }
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

/// ConfiguredRule: apply each user-supplied rule.
pub(crate) fn apply_configured_rules(
    rules: &[ExtraRule],
    modified_paths: &[PathBuf],
    discovered: &[SuiteRef],
    repo_root: &Path,
    dep_module_index: &BTreeMap<String, Vec<ModuleName>>,
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
    for rule in rules {
        let path_regex = match &rule.trigger {
            ExtraRuleTrigger::PathRegex { pattern, .. } => regex::Regex::new(pattern).ok(),
            _ => None,
        };
        let line_regex = rule
            .line_match
            .as_ref()
            .and_then(|m| regex::Regex::new(&m.pattern).ok());
        for path in modified_paths {
            let s = path.to_string_lossy();
            let path_captures = path_regex.as_ref().and_then(|r| r.captures(&s));
            let fires = match &rule.trigger {
                ExtraRuleTrigger::PathSuffix { suffix } => s.ends_with(suffix.as_str()),
                ExtraRuleTrigger::PathContains { fragment } => s.contains(fragment.as_str()),
                ExtraRuleTrigger::PathRegex { .. } => path_captures.is_some(),
            };
            if !fires {
                continue;
            }
            let path_caps_owned = match (&path_regex, &path_captures) {
                (Some(rx), Some(caps)) => captures_to_owned(rx, caps),
                _ => BTreeMap::new(),
            };

            let line_caps_iter: Vec<BTreeMap<String, String>> = if let Some(rx) = &line_regex {
                read_matched_lines(repo_root, path, rx)
            } else {
                vec![BTreeMap::new()]
            };

            for line_caps in &line_caps_iter {
                let merged = merge_captures(&path_caps_owned, line_caps);
                fire_rule_once(
                    rule,
                    path,
                    &merged,
                    discovered,
                    dep_module_index,
                    matcher,
                    out,
                );
            }
        }
    }
}

fn fire_rule_once(
    rule: &ExtraRule,
    path: &Path,
    captures: &BTreeMap<String, String>,
    discovered: &[SuiteRef],
    dep_module_index: &BTreeMap<String, Vec<ModuleName>>,
    matcher: &mut dyn SuiteMatcher,
    out: &mut BTreeMap<SuiteRef, Vec<SuiteInclusionReason>>,
) {
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
                triggering_path: path.to_path_buf(),
            });
    }
    for template in &rule.include_suite_templates {
        let module_name = match expand_template_named(template, captures) {
            Ok(m) => m,
            Err(_) => continue,
        };
        for suite in discovered
            .iter()
            .filter(|s| s.module.as_str() == module_name)
        {
            out.entry(suite.clone())
                .or_default()
                .push(SuiteInclusionReason::ConfiguredRule {
                    rule_name: rule.name.clone(),
                    triggering_path: path.to_path_buf(),
                });
        }
    }
    if rule.include_suite_for_dep_modules {
        let Some(dep_name) = captures.get("dep") else {
            return;
        };
        let Some(modules) = dep_module_index.get(dep_name) else {
            return;
        };
        if modules.is_empty() {
            return;
        }
        for suite in discovered {
            let refs = matcher.modules_referenced_in_suite(&suite.path, modules);
            if refs.is_empty() {
                continue;
            }
            out.entry(suite.clone())
                .or_default()
                .push(SuiteInclusionReason::ConfiguredRule {
                    rule_name: rule.name.clone(),
                    triggering_path: path.to_path_buf(),
                });
        }
    }
}

fn captures_to_owned(rx: &regex::Regex, caps: &regex::Captures<'_>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in rx.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            out.insert(name.to_owned(), m.as_str().to_owned());
        }
    }
    out
}

fn merge_captures(
    base: &BTreeMap<String, String>,
    extra: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = base.clone();
    for (k, v) in extra {
        out.insert(k.clone(), v.clone());
    }
    out
}

fn read_matched_lines(
    repo_root: &Path,
    path: &Path,
    line_regex: &regex::Regex,
) -> Vec<BTreeMap<String, String>> {
    let full = repo_root.join(path);
    let Ok(content) = fs::read_to_string(&full) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in content.lines() {
        if let Some(caps) = line_regex.captures(line) {
            out.push(captures_to_owned(line_regex, &caps));
        }
    }
    out
}

#[derive(Debug)]
pub struct TemplateError {
    pub placeholder: String,
}

/// Walk `{name}` placeholders in `template`, calling `push_value` to
/// append each replacement to the output. Literal text and an unmatched
/// `{` pass through. `push_value` errors when a placeholder cannot be
/// resolved.
fn substitute_placeholders(
    template: &str,
    mut push_value: impl FnMut(&str, &mut String) -> Result<(), TemplateError>,
) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push('{');
            rest = after;
            continue;
        };
        let name = &after[..end];
        push_value(name, &mut out)?;
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn expand_template_named(
    template: &str,
    captures: &BTreeMap<String, String>,
) -> Result<String, TemplateError> {
    substitute_placeholders(template, |name, out| {
        let v = captures.get(name).ok_or_else(|| TemplateError {
            placeholder: name.to_owned(),
        })?;
        out.push_str(v);
        Ok(())
    })
}

/// Validate that every `{name}` placeholder in `template` appears in
/// `allowed_captures`. Used at config-load time so unknown placeholders
/// fail fast.
pub fn validate_template_placeholders(
    template: &str,
    allowed_captures: &[String],
) -> Result<(), TemplateError> {
    substitute_placeholders(template, |name, _out| {
        if allowed_captures.iter().any(|c| c == name) {
            Ok(())
        } else {
            Err(TemplateError {
                placeholder: name.to_owned(),
            })
        }
    })
    .map(|_| ())
}
