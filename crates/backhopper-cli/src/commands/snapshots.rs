// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::str::FromStr;

use serde::Serialize;
use time::OffsetDateTime;

use backhopper_core::Error as CoreError;
use backhopper_core::Snapshot;
use backhopper_core::config::{Config, Language, Project};
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::snapshot::{FunArity, Module, SnapshotHeader, Visibility, state};
use backhopper_core::snapshot::format;
use backhopper_core::store::{Mutable, SnapshotStore};
use backhopper_elixir::ElixirExtractor;
use backhopper_erlang::ErlangExtractor;

use crate::cli::{GlobalArgs, SnapshotsCmd};
use crate::commands::context::{load_config, open_store_mut, open_store_read};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render, render_with_exit};

#[derive(Debug, Serialize)]
struct DiscoverPayload {
    project: String,
    captured: usize,
    skipped: usize,
    failed: Vec<DiscoverFailure>,
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ignored_non_tag_refs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DiscoverFailure {
    tag: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct ListPayload {
    project: String,
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ListTagsEntry {
    project: String,
    tags_without_snapshots: Vec<String>,
}

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

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DiffPayload {
    pub project: String,
    pub from: String,
    pub to: String,
    pub modules_added: Vec<String>,
    pub modules_removed: Vec<String>,
    pub exports_added: Vec<QualifiedFunArity>,
    pub exports_removed: Vec<QualifiedFunArity>,
    pub types_added: Vec<QualifiedTypeArity>,
    pub types_removed: Vec<QualifiedTypeArity>,
    pub callbacks_added: Vec<QualifiedFunArity>,
    pub callbacks_removed: Vec<QualifiedFunArity>,
    pub headers_added: Vec<String>,
    pub headers_removed: Vec<String>,
    pub records_added: Vec<QualifiedRecord>,
    pub records_removed: Vec<QualifiedRecord>,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct QualifiedFunArity {
    pub module: String,
    pub fun_arity: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct QualifiedTypeArity {
    pub module: String,
    pub type_arity: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct QualifiedRecord {
    pub header: String,
    pub record: String,
}

pub fn handle(args: &GlobalArgs, cmd: SnapshotsCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        SnapshotsCmd::ListTags { project } => list_tags(args, &cfg, project),
        SnapshotsCmd::Generate {
            project,
            no_remote_check: _,
            dry_run,
        } => generate(args, &cfg, project, dry_run),
        SnapshotsCmd::List { project } => list(args, &cfg, project),
        SnapshotsCmd::Show { project, tag } => show(args, &cfg, project, tag),
        SnapshotsCmd::Verify { project, tag } => verify(args, &cfg, project, tag),
        SnapshotsCmd::Rebuild {
            project,
            tag,
            dry_run,
        } => rebuild(args, &cfg, project, tag, dry_run),
        SnapshotsCmd::Lookup {
            project,
            tag,
            mfa,
            include_hidden,
        } => lookup(args, &cfg, project, tag, mfa, include_hidden),
        SnapshotsCmd::Modules {
            project,
            tag,
            include_hidden,
        } => modules(args, &cfg, project, tag, include_hidden),
        SnapshotsCmd::Exports {
            project,
            tag,
            module,
        } => exports(args, &cfg, project, tag, module),
        SnapshotsCmd::Diff { project, from, to } => diff(args, &cfg, project, from, to),
    }
}

fn list_tags(args: &GlobalArgs, cfg: &Config, project: Option<ProjectName>) -> CliResult<i32> {
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p).map_err(|e| CliError::Core(e.into()))?],
        None => cfg.projects.iter().collect(),
    };
    let store = open_store_read(args, cfg)?;
    let mut payloads: Vec<ListTagsEntry> = Vec::new();
    for p in projects {
        let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
        let listing = repo.list_tag_refs().map_err(|e| CliError::Core(e.into()))?;
        let mut pending: Vec<String> = Vec::new();
        for tag in listing.tags {
            if !store.has(&p.name, &tag) {
                pending.push(tag.to_string());
            }
        }
        payloads.push(ListTagsEntry {
            project: p.name.to_string(),
            tags_without_snapshots: pending,
        });
    }
    let ctx = OutputContext::new(args.formatter, "snapshots list_tags");
    render(&ctx, &payloads, |w| {
        for p in &payloads {
            writeln!(
                w,
                "{}: {} tag(s) without snapshots",
                p.project,
                p.tags_without_snapshots.len()
            )?;
            for tag in &p.tags_without_snapshots {
                writeln!(w, "  {}", tag)?;
            }
        }
        Ok(())
    })?;
    Ok(0)
}

fn generate(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    dry_run: bool,
) -> CliResult<i32> {
    let store = open_store_mut(args, cfg)?;
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p).map_err(|e| CliError::Core(e.into()))?],
        None => cfg.projects.iter().collect(),
    };
    let mut payloads = Vec::new();
    for p in projects {
        payloads.push(generate_one(p, &store, dry_run)?);
    }
    let ctx = OutputContext::new(args.formatter, "snapshots generate");
    render(&ctx, &payloads, |w| {
        for p in &payloads {
            writeln!(
                w,
                "{}: captured {}, skipped {}, failed {}",
                p.project,
                p.captured,
                p.skipped,
                p.failed.len()
            )?;
            if !p.ignored_non_tag_refs.is_empty() {
                writeln!(
                    w,
                    "  ignored {} ref{} that did not parse as a tag name",
                    p.ignored_non_tag_refs.len(),
                    if p.ignored_non_tag_refs.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                )?;
            }
        }
        Ok(())
    })?;
    Ok(0)
}

fn generate_one(
    p: &Project,
    store: &SnapshotStore<Mutable>,
    dry_run: bool,
) -> CliResult<DiscoverPayload> {
    let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let listing = repo.list_tag_refs().map_err(|e| CliError::Core(e.into()))?;
    let mut captured = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<DiscoverFailure> = Vec::new();
    let mut captured_tags = Vec::new();
    for tag in listing.tags {
        if store.has(&p.name, &tag) {
            skipped += 1;
            continue;
        }
        match build_snapshot(p, &repo, &tag) {
            Ok(snapshot) => {
                if !dry_run {
                    if let Err(e) = store.write(&snapshot) {
                        failed.push(DiscoverFailure {
                            tag: tag.to_string(),
                            reason: e.to_string(),
                        });
                        continue;
                    }
                }
                captured += 1;
                captured_tags.push(tag.to_string());
            }
            Err(e) => {
                failed.push(DiscoverFailure {
                    tag: tag.to_string(),
                    reason: e.to_string(),
                });
            }
        }
    }
    Ok(DiscoverPayload {
        project: p.name.to_string(),
        captured,
        skipped,
        failed,
        tags: captured_tags,
        ignored_non_tag_refs: listing.skipped,
    })
}

fn build_snapshot(
    p: &Project,
    repo: &GitRepo,
    tag: &TagName,
) -> CliResult<Snapshot<state::Canonical>> {
    let commit = repo
        .resolve_tag(tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let scan_paths = p.scan_paths.clone();
    let blobs = repo
        .read_paths_at_commit(&commit, |path| matches_any(path, &scan_paths))
        .map_err(|e| CliError::Core(e.into()))?;
    let files: Vec<(PathBuf, Vec<u8>)> = blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
    let extracted = match p.language {
        Language::Erlang => {
            let extractor =
                ErlangExtractor::new(p.public_modules.clone(), p.internal_modules.clone());
            let r = extractor
                .extract_source(files)
                .map_err(|e| CliError::Other(e.to_string()))?;
            (r.modules, r.headers)
        }
        Language::Elixir => {
            let extractor =
                ElixirExtractor::new(p.public_modules.clone(), p.internal_modules.clone());
            let r = extractor
                .extract_source(files)
                .map_err(|e| CliError::Other(e.to_string()))?;
            (r.modules, r.headers)
        }
    };
    let branch = repo
        .branches_containing(&commit)
        .ok()
        .and_then(|v| v.into_iter().next());
    let header = SnapshotHeader {
        project: p.name.clone(),
        tag: tag.clone(),
        branch,
        commit,
        scanned_paths: scan_paths,
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::now_utc(),
    };
    let (modules, headers) = extracted;
    let snapshot = Snapshot::from_extracted(header, modules, headers).into_canonical();
    Ok(snapshot)
}

fn matches_any(path: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        if glob_match(pat, path) {
            return true;
        }
    }
    false
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let path = path.trim_start_matches("./");
    if let Some((dir, suffix)) = pattern.split_once("**/") {
        if path.starts_with(dir.trim_end_matches('/')) || dir.is_empty() {
            let rest = path
                .strip_prefix(dir.trim_end_matches('/'))
                .map(|s| s.trim_start_matches('/'))
                .unwrap_or(path);
            return suffix_match(suffix, rest);
        }
    }
    if let Some((dir, suffix)) = pattern.split_once("/*") {
        if path.starts_with(dir) {
            let rest = path.strip_prefix(dir).unwrap_or(path);
            let rest = rest.trim_start_matches('/');
            return suffix_match(suffix.trim_start_matches('/'), rest);
        }
    }
    pattern == path
}

fn suffix_match(suffix: &str, path: &str) -> bool {
    if let Some(ext) = suffix.strip_prefix("*.") {
        path.rsplit_once('.').is_some_and(|(_, e)| e == ext)
    } else {
        suffix.is_empty() || path.ends_with(suffix)
    }
}

fn list(args: &GlobalArgs, cfg: &Config, project: ProjectName) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let tags = store
        .list_tags(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    let payload = ListPayload {
        project: project.to_string(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    };
    let ctx = OutputContext::new(args.formatter, "snapshots list");
    render(&ctx, &payload, |w| {
        for t in &payload.tags {
            writeln!(w, "{}", t)?;
        }
        Ok(())
    })?;
    Ok(0)
}

fn show(args: &GlobalArgs, cfg: &Config, project: ProjectName, tag: TagName) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let ctx = OutputContext::new(args.formatter, "snapshots show");
    render(&ctx, &snapshot, |w| {
        let text =
            format::to_string(&snapshot).map_err(|e| CliError::Core(CoreError::Snapshot(e)))?;
        write!(w, "{}", text)?;
        Ok(())
    })?;
    Ok(0)
}

fn verify(args: &GlobalArgs, cfg: &Config, project: ProjectName, tag: TagName) -> CliResult<i32> {
    let p = cfg
        .project(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let regenerated = build_snapshot(p, &repo, &tag)?;
    let store = open_store_read(args, cfg)?;
    let stored = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let matches = regenerated.modules() == stored.modules()
        && regenerated.headers() == stored.headers()
        && regenerated.header().commit == stored.header().commit;
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "matches": matches,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots verify");
    let exit = if matches { 0 } else { 1 };
    render_with_exit(&ctx, &payload, exit, |w| {
        if matches {
            writeln!(w, "ok: {} {}", project, tag)?;
        } else {
            writeln!(w, "drift: {} {}", project, tag)?;
        }
        Ok(())
    })
}

fn rebuild(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    dry_run: bool,
) -> CliResult<i32> {
    let p = cfg
        .project(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let snapshot = build_snapshot(p, &repo, &tag)?;
    let store = open_store_mut(args, cfg)?;
    if !dry_run {
        let _ = store.delete(&project, &tag);
        store
            .write(&snapshot)
            .map_err(|e| CliError::Core(e.into()))?;
    }
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "rebuilt": !dry_run,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots rebuild");
    render(&ctx, &payload, |w| {
        writeln!(w, "rebuilt {} {}", project, tag)?;
        Ok(())
    })?;
    Ok(0)
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
        let module = snapshot.module_named(&mfa.module);
        let allowed = match module {
            Some(m) => include_hidden || matches!(m.visibility, Visibility::Public),
            None => true,
        };
        let target = FunArity {
            name: mfa.function.clone(),
            arity: mfa.arity,
        };
        let found = allowed && module.is_some_and(|m| m.exports.binary_search(&target).is_ok());
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
    let ctx = OutputContext::new(args.formatter, "snapshots lookup");
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
    let ctx = OutputContext::new(args.formatter, "snapshots modules");
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
    let m: Option<&Module> = snapshot.module_named(&mod_name);
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
    let ctx = OutputContext::new(args.formatter, "snapshots exports");
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
    let ctx = OutputContext::new(args.formatter, "snapshots diff");
    render(&ctx, &result, |w| {
        render_diff_text(w, &result).map_err(CliError::from)
    })?;
    Ok(0)
}

pub fn render_diff_text<W: Write + ?Sized>(w: &mut W, d: &DiffPayload) -> io::Result<()> {
    for name in &d.modules_removed {
        writeln!(w, "removed module {name}")?;
    }
    for name in &d.modules_added {
        writeln!(w, "added module {name}")?;
    }
    for q in &d.exports_removed {
        writeln!(w, "removed export {}:{}", q.module, q.fun_arity)?;
    }
    for q in &d.exports_added {
        writeln!(w, "added export {}:{}", q.module, q.fun_arity)?;
    }
    for q in &d.callbacks_removed {
        writeln!(w, "removed callback {}:{}", q.module, q.fun_arity)?;
    }
    for q in &d.callbacks_added {
        writeln!(w, "added callback {}:{}", q.module, q.fun_arity)?;
    }
    for q in &d.types_removed {
        writeln!(w, "removed type {}:{}", q.module, q.type_arity)?;
    }
    for q in &d.types_added {
        writeln!(w, "added type {}:{}", q.module, q.type_arity)?;
    }
    for h in &d.headers_removed {
        writeln!(w, "removed header {h}")?;
    }
    for h in &d.headers_added {
        writeln!(w, "added header {h}")?;
    }
    for r in &d.records_removed {
        writeln!(w, "removed record {}:{}", r.header, r.record)?;
    }
    for r in &d.records_added {
        writeln!(w, "added record {}:{}", r.header, r.record)?;
    }
    Ok(())
}

pub fn compute_diff(a: &Snapshot<state::Canonical>, b: &Snapshot<state::Canonical>) -> DiffPayload {
    let a_names: BTreeSet<&ModuleName> = a.modules().iter().map(|m| &m.name).collect();
    let b_names: BTreeSet<&ModuleName> = b.modules().iter().map(|m| &m.name).collect();
    let modules_added: Vec<String> = b_names
        .difference(&a_names)
        .map(|n| n.to_string())
        .collect();
    let modules_removed: Vec<String> = a_names
        .difference(&b_names)
        .map(|n| n.to_string())
        .collect();
    let mut exports_added = Vec::new();
    let mut exports_removed = Vec::new();
    let mut types_added = Vec::new();
    let mut types_removed = Vec::new();
    let mut callbacks_added = Vec::new();
    let mut callbacks_removed = Vec::new();
    for name in a_names.union(&b_names) {
        let module_str = name.to_string();
        diff_named_set(
            module_str.as_str(),
            module_exports(a, name),
            module_exports(b, name),
            &mut exports_added,
            &mut exports_removed,
            |module, fa| QualifiedFunArity {
                module: module.into(),
                fun_arity: fa,
            },
        );
        diff_named_set(
            module_str.as_str(),
            module_callbacks(a, name),
            module_callbacks(b, name),
            &mut callbacks_added,
            &mut callbacks_removed,
            |module, fa| QualifiedFunArity {
                module: module.into(),
                fun_arity: fa,
            },
        );
        diff_named_set(
            module_str.as_str(),
            module_types(a, name),
            module_types(b, name),
            &mut types_added,
            &mut types_removed,
            |module, ta| QualifiedTypeArity {
                module: module.into(),
                type_arity: ta,
            },
        );
    }
    let a_headers: BTreeSet<&str> = a.headers().iter().map(|h| h.path.as_str()).collect();
    let b_headers: BTreeSet<&str> = b.headers().iter().map(|h| h.path.as_str()).collect();
    let headers_added: Vec<String> = b_headers
        .difference(&a_headers)
        .map(|s| (*s).to_owned())
        .collect();
    let headers_removed: Vec<String> = a_headers
        .difference(&b_headers)
        .map(|s| (*s).to_owned())
        .collect();
    let mut records_added = Vec::new();
    let mut records_removed = Vec::new();
    for path in a_headers.union(&b_headers) {
        diff_named_set(
            path,
            hrl_records(a, path),
            hrl_records(b, path),
            &mut records_added,
            &mut records_removed,
            |header, record| QualifiedRecord {
                header: header.into(),
                record,
            },
        );
    }
    DiffPayload {
        project: a.header().project.to_string(),
        from: a.header().tag.to_string(),
        to: b.header().tag.to_string(),
        modules_added,
        modules_removed,
        exports_added,
        exports_removed,
        types_added,
        types_removed,
        callbacks_added,
        callbacks_removed,
        headers_added,
        headers_removed,
        records_added,
        records_removed,
    }
}

fn diff_named_set<T, F>(
    parent: &str,
    a: BTreeSet<String>,
    b: BTreeSet<String>,
    added: &mut Vec<T>,
    removed: &mut Vec<T>,
    qualify: F,
) where
    F: Fn(&str, String) -> T,
{
    for s in b.difference(&a) {
        added.push(qualify(parent, s.clone()));
    }
    for s in a.difference(&b) {
        removed.push(qualify(parent, s.clone()));
    }
}

fn module_exports(s: &Snapshot<state::Canonical>, name: &ModuleName) -> BTreeSet<String> {
    s.module_named(name)
        .map(|m| {
            m.exports
                .iter()
                .map(|fa| format!("{}/{}", fa.name, fa.arity))
                .collect()
        })
        .unwrap_or_default()
}

fn module_callbacks(s: &Snapshot<state::Canonical>, name: &ModuleName) -> BTreeSet<String> {
    s.module_named(name)
        .map(|m| {
            m.callbacks
                .iter()
                .map(|cb| format!("{}/{}", cb.name, cb.arity))
                .collect()
        })
        .unwrap_or_default()
}

fn module_types(s: &Snapshot<state::Canonical>, name: &ModuleName) -> BTreeSet<String> {
    s.module_named(name)
        .map(|m| {
            m.types
                .iter()
                .map(|t| format!("{}/{}", t.name, t.arity))
                .chain(
                    m.export_types
                        .iter()
                        .map(|t| format!("{}/{}", t.name, t.arity)),
                )
                .chain(m.opaques.iter().map(|t| format!("{}/{}", t.name, t.arity)))
                .collect()
        })
        .unwrap_or_default()
}

fn hrl_records(s: &Snapshot<state::Canonical>, path: &str) -> BTreeSet<String> {
    s.headers()
        .iter()
        .find(|h| h.path == path)
        .map(|h| h.records.iter().map(|r| r.name.to_string()).collect())
        .unwrap_or_default()
}