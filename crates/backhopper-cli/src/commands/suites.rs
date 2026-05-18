//! `backhopper suites ...` handler.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use backhopper_core::app_src::{AppSrcSpec, DiscoveryWarning, discover};
use backhopper_core::model::names::ApplicationName;
use backhopper_core::suites::{
    BuildSystem, PlanInput, SuiteInclusionReason, SuitePlan, derive_library_apps, plan_with_matcher,
};
use backhopper_xref::{is_suite_module, suites_referencing, suites_referencing_mfas};
use backhopper_xref_reader::AstSuiteMatcher;

use crate::cli::suites::SuitesPlanArgs;
use crate::cli::{GlobalArgs, SuitesCmd};
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
    let input = PlanInput {
        repo_root: args.repo_dir_path.clone(),
        modified_paths,
        apps: discovery.specs,
        library_apps,
        extra_rules: vec![],
    };
    let mut matcher = AstSuiteMatcher::new();
    let result = plan_with_matcher(&input, &mut matcher);
    render_plan(global, &result, build_system)?;
    Ok(0)
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
        if let Some(hint) = build_system_hint(build_system, result) {
            writeln!(w)?;
            writeln!(w, "{hint}")?;
        }
        Ok(())
    })?;
    Ok(())
}

fn build_system_hint(bs: BuildSystem, result: &SuitePlan) -> Option<&'static str> {
    if result.is_empty() {
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
    }
}
