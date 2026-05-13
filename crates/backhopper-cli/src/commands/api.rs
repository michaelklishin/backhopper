use std::collections::BTreeSet;
use std::str::FromStr;

use serde::Serialize;

use backhopper_core::Error as CoreError;
use backhopper_core::Snapshot;
use backhopper_core::config::Config;
use backhopper_core::model::names::{Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::snapshot::{Module, Visibility, state};

use crate::cli::{ApiCmd, GlobalArgs};
use crate::commands::context::{load_config, open_store_read};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render, render_with_exit};

#[derive(Debug, Serialize)]
struct LookupResult {
    mfa: String,
    found: bool,
    visibility: Option<String>,
}

#[derive(Debug, Serialize)]
struct LookupPayload {
    project: String,
    tag: String,
    results: Vec<LookupResult>,
}

#[derive(Debug, Serialize)]
struct ModulesPayload {
    project: String,
    tag: String,
    modules: Vec<ModuleSummary>,
    headers: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ModuleSummary {
    name: String,
    visibility: String,
    exports: usize,
    callbacks: usize,
}

pub fn handle(args: &GlobalArgs, cmd: ApiCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        ApiCmd::Lookup {
            project,
            tag,
            mfa,
            include_hidden,
        } => lookup(args, &cfg, project, tag, mfa, include_hidden),
        ApiCmd::Modules {
            project,
            tag,
            include_hidden,
        } => modules(args, &cfg, project, tag, include_hidden),
        ApiCmd::Exports {
            project,
            tag,
            module,
        } => exports(args, &cfg, project, tag, module),
        ApiCmd::Diff { project, from, to } => diff(args, &cfg, project, from, to),
    }
}

fn lookup(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    mfas: Vec<Mfa>,
    include_hidden: bool,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let mut results = Vec::with_capacity(mfas.len());
    let mut all_found = true;
    for mfa in &mfas {
        let module = snapshot.modules().iter().find(|m| m.name == mfa.module);
        let allowed = match module {
            Some(m) => include_hidden || matches!(m.visibility, Visibility::Public),
            None => true,
        };
        let found = allowed && snapshot.lookup_export(&mfa.module, &mfa.function, mfa.arity);
        if !found {
            all_found = false;
        }
        results.push(LookupResult {
            mfa: mfa.to_string(),
            found,
            visibility: module.map(|m| m.visibility.keyword().to_owned()),
        });
    }
    let payload = LookupPayload {
        project: project.to_string(),
        tag: tag.to_string(),
        results,
    };
    let ctx = OutputContext::new(args.formatter, "api lookup");
    let exit = if all_found { 0 } else { 1 };
    render_with_exit(&ctx, &payload, exit, |w| {
        for r in &payload.results {
            writeln!(
                w,
                "{}\t{}\t{}",
                r.mfa,
                if r.found { "found" } else { "missing" },
                r.visibility.as_deref().unwrap_or("-")
            )?;
        }
        Ok(())
    })
}

fn modules(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    include_hidden: bool,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let mut payload = ModulesPayload {
        project: project.to_string(),
        tag: tag.to_string(),
        modules: Vec::new(),
        headers: snapshot.headers().iter().map(|h| h.path.clone()).collect(),
    };
    for m in snapshot.modules() {
        if !include_hidden && m.visibility != Visibility::Public {
            continue;
        }
        payload.modules.push(ModuleSummary {
            name: m.name.to_string(),
            visibility: m.visibility.keyword().to_owned(),
            exports: m.exports.len(),
            callbacks: m.callbacks.len(),
        });
    }
    let ctx = OutputContext::new(args.formatter, "api modules");
    render(&ctx, &payload, |w| {
        for m in &payload.modules {
            writeln!(
                w,
                "{}\t{}\t{} exports\t{} callbacks",
                m.name, m.visibility, m.exports, m.callbacks
            )?;
        }
        for h in &payload.headers {
            writeln!(w, "{}\theader", h)?;
        }
        Ok(())
    })?;
    Ok(0)
}

fn exports(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    module: String,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let mod_name = ModuleName::from_str(&module).map_err(|e| CliError::Core(CoreError::Name(e)))?;
    let m: Option<&Module> = snapshot.modules().iter().find(|m| m.name == mod_name);
    let exports: Vec<String> = m
        .map(|m| {
            m.exports
                .iter()
                .map(|fa| format!("{}/{}", fa.name, fa.arity))
                .collect()
        })
        .unwrap_or_default();
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "module":  module,
        "exports": exports,
    });
    let ctx = OutputContext::new(args.formatter, "api exports");
    render(&ctx, &payload, |w| {
        for e in &exports {
            writeln!(w, "{}", e)?;
        }
        Ok(())
    })?;
    Ok(if m.is_some() { 0 } else { 1 })
}

fn diff(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    from: TagName,
    to: TagName,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let a = store
        .read(&project, &from)
        .map_err(|e| CliError::Core(e.into()))?;
    let b = store
        .read(&project, &to)
        .map_err(|e| CliError::Core(e.into()))?;
    let result = compute_diff(&a, &b);
    let ctx = OutputContext::new(args.formatter, "api diff");
    render(&ctx, &result, |w| {
        for added in &result.exports_added {
            writeln!(w, "+ export {} {}", added.module, added.fun_arity)?;
        }
        for removed in &result.exports_removed {
            writeln!(w, "- export {} {}", removed.module, removed.fun_arity)?;
        }
        Ok(())
    })?;
    Ok(0)
}

#[derive(Debug, Serialize)]
struct DiffPayload {
    project: String,
    from: String,
    to: String,
    modules_added: Vec<String>,
    modules_removed: Vec<String>,
    exports_added: Vec<DiffExport>,
    exports_removed: Vec<DiffExport>,
}

#[derive(Debug, Serialize)]
struct DiffExport {
    module: String,
    fun_arity: String,
}

fn compute_diff(a: &Snapshot<state::Canonical>, b: &Snapshot<state::Canonical>) -> DiffPayload {
    let a_modules: BTreeSet<String> = a.modules().iter().map(|m| m.name.to_string()).collect();
    let b_modules: BTreeSet<String> = b.modules().iter().map(|m| m.name.to_string()).collect();
    let modules_added: Vec<_> = b_modules.difference(&a_modules).cloned().collect();
    let modules_removed: Vec<_> = a_modules.difference(&b_modules).cloned().collect();
    let mut exports_added = Vec::new();
    let mut exports_removed = Vec::new();
    let module_names: BTreeSet<_> = a_modules.union(&b_modules).cloned().collect();
    for name in module_names {
        let a_exports: BTreeSet<String> = a
            .modules()
            .iter()
            .find(|m| m.name.as_str() == name)
            .map(|m| {
                m.exports
                    .iter()
                    .map(|fa| format!("{}/{}", fa.name, fa.arity))
                    .collect()
            })
            .unwrap_or_default();
        let b_exports: BTreeSet<String> = b
            .modules()
            .iter()
            .find(|m| m.name.as_str() == name)
            .map(|m| {
                m.exports
                    .iter()
                    .map(|fa| format!("{}/{}", fa.name, fa.arity))
                    .collect()
            })
            .unwrap_or_default();
        for added in b_exports.difference(&a_exports) {
            exports_added.push(DiffExport {
                module: name.clone(),
                fun_arity: added.clone(),
            });
        }
        for removed in a_exports.difference(&b_exports) {
            exports_removed.push(DiffExport {
                module: name.clone(),
                fun_arity: removed.clone(),
            });
        }
    }
    DiffPayload {
        project: a.header().project.to_string(),
        from: a.header().tag.to_string(),
        to: b.header().tag.to_string(),
        modules_added,
        modules_removed,
        exports_added,
        exports_removed,
    }
}
