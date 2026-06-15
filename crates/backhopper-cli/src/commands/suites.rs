// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper suites ...` handler.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use backhopper_core::Error as CoreError;
use backhopper_core::app_src::{AppSrcSpec, DiscoveryWarning, discover};
use backhopper_core::config::{Config, parse_suite_rules_toml};
use backhopper_core::model::names::{ApplicationName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::PinSpec;
use backhopper_core::suites::{
    BuildSystem, ExtraRule, PlanInput, SuiteInclusionReason, SuitePlan, derive_library_apps,
    plan_with_matcher,
};
use backhopper_xref::{is_suite_module, suites_referencing, suites_referencing_mfas};
use backhopper_xref_reader::AstSuiteMatcher;

use crate::cli::suites::SuitesPlanArgs;
use crate::cli::{GlobalArgs, SuitesCmd};
use crate::commands::context::{load_config, open_store_read};
use crate::commands::tree_source::build_xref;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

fn ctx(global: &GlobalArgs, command: &'static str) -> OutputContext {
    OutputContext::new(global.formatter, command)
}

pub fn handle(global: &GlobalArgs, cmd: SuitesCmd) -> CliResult<i32> {
    match cmd {
        SuitesCmd::ListForModules { tree, module } => {
            let xref = build_xref(&tree)?;
            let mods: BTreeSet<_> = module.into_iter().collect();
            let suites = suites_referencing(&xref, &mods, is_suite_module);
            render(&ctx(global, "suites list_for_modules"), &suites, |w| {
                for s in &suites {
                    match &s.application {
                        Some(a) => writeln!(w, "{}/{}", a, s.module)?,
                        None => writeln!(w, "{}", s.module)?,
                    }
                }
                Ok(())
            })?;
            Ok(0)
        }
        SuitesCmd::ListForMfas { tree, mfa } => {
            let xref = build_xref(&tree)?;
            let mfas: BTreeSet<_> = mfa.into_iter().collect();
            let suites = suites_referencing_mfas(&xref, &mfas, is_suite_module);
            render(&ctx(global, "suites list_for_mfas"), &suites, |w| {
                for s in &suites {
                    match &s.application {
                        Some(a) => writeln!(w, "{}/{}", a, s.module)?,
                        None => writeln!(w, "{}", s.module)?,
                    }
                }
                Ok(())
            })?;
            Ok(0)
        }
        SuitesCmd::ListCallersOf { tree, mfa } => {
            let xref = build_xref(&tree)?;
            let callers = xref.called_by(&mfa, false);
            render(&ctx(global, "suites list_callers_of"), &callers, |w| {
                writeln!(w, "callers of {}:", callers.target)?;
                for c in &callers.entries {
                    writeln!(w, "  {}", c.caller)?;
                }
                Ok(())
            })?;
            Ok(0)
        }
        SuitesCmd::Plan(args) => run_suites_plan(global, args),
    }
}

fn run_suites_plan(global: &GlobalArgs, args: SuitesPlanArgs) -> CliResult<i32> {
    let modified_paths = collect_modified_paths(&args)?;
    let discovery = discover(&args.repo_dir_path);
    for w in &discovery.warnings {
        log_discovery_warning(w);
    }
    let build_system = BuildSystem::detect(&args.repo_dir_path);
    let library_apps = resolve_library_apps(&args, &discovery.specs);
    // A broken config must not silently degrade to "no extra rules":
    // workflows rely on configured rules firing. Having no config at
    // all stays silent; there are no rules to lose.
    let cfg = match load_config(global) {
        Ok(c) => Some(c),
        Err(e) if global.config_file_path.is_some() => return Err(e),
        Err(CliError::ConfigNotFound { .. }) => None,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "config load failed; configured suite rules will not fire"
            );
            None
        }
    };
    let mut extra_rules = cfg
        .as_ref()
        .map(|c| c.suite_rules.clone())
        .unwrap_or_default();
    // Rules from the file are unioned with the config's, validated the
    // same way, so a rule can be passed per-invocation.
    if let Some(path) = &args.extra_rules_file_path {
        let text = fs::read_to_string(path)?;
        extra_rules.extend(parse_suite_rules_toml(&text).map_err(CoreError::from)?);
    }
    let dep_module_index = cfg
        .as_ref()
        .map(|c| build_dep_module_index(global, c, &extra_rules))
        .unwrap_or_default();
    let input = PlanInput {
        repo_root: args.repo_dir_path,
        modified_paths,
        apps: discovery.specs,
        library_apps,
        extra_rules,
        implementer_index: BTreeMap::new(),
        dep_module_index,
    };
    let mut matcher = AstSuiteMatcher::new();
    let result = plan_with_matcher(&input, &mut matcher);
    render_plan(global, &result, build_system)?;
    Ok(0)
}

fn build_dep_module_index(
    global: &GlobalArgs,
    cfg: &Config,
    extra_rules: &[ExtraRule],
) -> BTreeMap<String, Vec<ModuleName>> {
    if !extra_rules.iter().any(|r| r.include_suite_for_dep_modules) {
        return BTreeMap::new();
    }
    let Ok(store) = open_store_read(global, cfg) else {
        return BTreeMap::new();
    };
    let mut out: BTreeMap<String, Vec<ModuleName>> = BTreeMap::new();
    for series in &cfg.series {
        for spec in &series.pins {
            let (project, tag): (ProjectName, TagName) = match spec {
                PinSpec::Literal { project, tag } => (project.clone(), tag.clone()),
                PinSpec::Pattern { .. } => match spec.resolve(&store) {
                    Ok(pin) => (pin.project, pin.tag),
                    Err(_) => continue,
                },
                PinSpec::SelfRef { .. } => continue,
            };
            if out.contains_key(project.as_str()) {
                continue;
            }
            let Ok(snap) = store.read(&project, &tag) else {
                continue;
            };
            let modules: Vec<ModuleName> = snap.modules().iter().map(|m| m.name.clone()).collect();
            out.insert(project.to_string(), modules);
        }
    }
    out
}

fn log_discovery_warning(w: &DiscoveryWarning) {
    match w {
        DiscoveryWarning::ParseFailed { path, reason } => {
            tracing::warn!(
                path = %path.display(),
                reason = %reason,
                ".app.src parse failed; skipping",
            );
        }
        DiscoveryWarning::ReadFailed { path, reason } => {
            tracing::warn!(
                path = %path.display(),
                reason = %reason,
                ".app.src read failed; skipping",
            );
        }
    }
}

fn resolve_library_apps(args: &SuitesPlanArgs, apps: &[AppSrcSpec]) -> Vec<ApplicationName> {
    let mut out: BTreeSet<ApplicationName> = args.library_app.iter().cloned().collect();
    if args.auto_library_apps {
        out.extend(derive_library_apps(apps));
    }
    out.into_iter().collect()
}

fn collect_modified_paths(args: &SuitesPlanArgs) -> CliResult<Vec<PathBuf>> {
    if let Some(path) = &args.modified_paths_file_path {
        return read_modified_paths_from_file(path);
    }
    if !args.modified_path.is_empty() {
        return Ok(args.modified_path.clone());
    }
    Err(CliError::InvalidInput(
        "supply either --modified-paths-file-path or one or more --modified-path".into(),
    ))
}

fn read_modified_paths_from_file(path: &Path) -> CliResult<Vec<PathBuf>> {
    let text = if path == Path::new("-") {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)?;
        s
    } else {
        fs::read_to_string(path)?
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(PathBuf::from(trimmed));
    }
    Ok(out)
}

fn render_plan(
    global: &GlobalArgs,
    result: &SuitePlan,
    build_system: BuildSystem,
) -> CliResult<()> {
    render(&ctx(global, "suites plan"), result, |w| {
        if result.is_empty() {
            writeln!(w, "no suites selected")?;
        } else {
            writeln!(w, "{} suite(s) selected", result.len())?;
            for entry in &result.entries {
                writeln!(w, "  {}/{}", entry.suite.application, entry.suite.module)?;
                for reason in &entry.reasons {
                    writeln!(w, "    - {}", reason_label(reason))?;
                }
            }
        }
        for u in &result.uncovered {
            writeln!(
                w,
                "uncovered: {} ({} modified module(s), {} suite(s) discovered, 0 selected)",
                u.application, u.modified_modules, u.discovered_suites
            )?;
            if u.suites.is_empty() {
                writeln!(w, "  this application has no test suites")?;
            } else {
                let names: Vec<&str> = u.suites.iter().map(|m| m.as_str()).collect();
                writeln!(w, "  candidates: {}", names.join(", "))?;
            }
        }
        if result.unattributed_paths > 0 {
            writeln!(
                w,
                "{} modified path(s) resolve to no application",
                result.unattributed_paths
            )?;
        }
        if let Some(hint) = build_system_hint(build_system, result) {
            writeln!(w)?;
            writeln!(w, "{hint}")?;
        }
        Ok(())
    })?;
    Ok(())
}

fn build_system_hint(bs: BuildSystem, result: &SuitePlan) -> Option<&'static str> {
    // Uncovered candidates want the run hint just as much as
    // selected entries do.
    if result.is_empty() && result.uncovered.is_empty() {
        return None;
    }
    match bs {
        BuildSystem::Rebar3 => Some(
            "detected rebar3: run each suite with `rebar3 ct --dir <app>/test --suite <module>`",
        ),
        BuildSystem::ErlangMk => {
            Some("detected erlang.mk: run each suite with `make -C <app_root> ct-<suite_basename>`")
        }
        BuildSystem::Unknown => None,
    }
}

fn reason_label(r: &SuiteInclusionReason) -> String {
    match r {
        SuiteInclusionReason::TestModified { path } => {
            format!("TestModified ({})", path.display())
        }
        SuiteInclusionReason::UnitOrPropSweep {
            application,
            triggering_modules,
        } => {
            let names: Vec<&str> = triggering_modules.iter().map(|m| m.as_str()).collect();
            format!(
                "UnitOrPropSweep (app={application}, modules={})",
                names.join(",")
            )
        }
        SuiteInclusionReason::SameAppCaller {
            application,
            module,
        } => format!("SameAppCaller (app={application}, module={module})"),
        SuiteInclusionReason::CrossAppCaller {
            library_application,
            module,
        } => format!("CrossAppCaller (library_app={library_application}, module={module})"),
        SuiteInclusionReason::ConfiguredRule {
            rule_name,
            triggering_path,
        } => format!(
            "ConfiguredRule (name={rule_name}, path={})",
            triggering_path.display()
        ),
        SuiteInclusionReason::BehaviourImplementerSweep {
            behaviour,
            implementer,
        } => {
            format!("BehaviourImplementerSweep (behaviour={behaviour}, implementer={implementer})")
        }
    }
}
