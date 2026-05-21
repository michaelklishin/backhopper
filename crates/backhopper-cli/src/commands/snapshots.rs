// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;
use std::str::FromStr;

use serde::Serialize;
use time::OffsetDateTime;

use backhopper_core::Error as CoreError;
use backhopper_core::Snapshot;
use backhopper_core::config::{Config, Language, Project, ProjectLayout};
use backhopper_core::git::{GitRepo, version_cmp};
use backhopper_core::model::names::{
    ApplicationName, Mfa, ModuleName, ProjectName, SeriesName, TagName,
};
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
            since,
        } => generate(args, &cfg, project, dry_run, since),
        SnapshotsCmd::List { project } => list(args, &cfg, project),
        SnapshotsCmd::Show {
            project,
            tag,
            module,
        } => show(args, &cfg, project, tag, module),
        SnapshotsCmd::Verify { project, tag, all } => verify(args, &cfg, project, tag, all),
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
            all_tags,
        } => lookup(args, &cfg, project, tag, mfa, include_hidden, all_tags),
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
        SnapshotsCmd::Diff {
            project,
            from,
            to,
            from_series,
            to_series,
        } => diff(args, &cfg, project, from, to, from_series, to_series),
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
    since: Option<TagName>,
) -> CliResult<i32> {
    let store = open_store_mut(args, cfg)?;
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p).map_err(|e| CliError::Core(e.into()))?],
        None => cfg.projects.iter().collect(),
    };
    let mut payloads = Vec::new();
    for p in projects {
        payloads.push(generate_one(p, &store, dry_run, since.as_ref())?);
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
    since: Option<&TagName>,
) -> CliResult<DiscoverPayload> {
    let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let listing = repo.list_tag_refs().map_err(|e| CliError::Core(e.into()))?;
    let tags = filter_tags_for_project(listing.tags, p, since);
    let mut captured = 0usize;
    let mut skipped = 0usize;
    let mut failed: Vec<DiscoverFailure> = Vec::new();
    let mut captured_tags = Vec::new();
    for tag in tags {
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

pub fn filter_tags_since(tags: Vec<TagName>, since: Option<&TagName>) -> Vec<TagName> {
    let Some(since) = since else {
        return tags;
    };
    tags.into_iter()
        .filter(|t| version_cmp(t.as_str(), since.as_str()) != Ordering::Greater)
        .collect()
}

/// Apply project-level tag filters (`tag_pattern`, `min_tag`,
/// `exclude_tag_markers`) plus the CLI `--since` filter. The CLI filter stacks
/// on top of project-level ones.
pub fn filter_tags_for_project(
    tags: Vec<TagName>,
    project: &Project,
    since: Option<&TagName>,
) -> Vec<TagName> {
    let mut out = tags;
    if let Some(glob) = &project.tag_pattern {
        out.retain(|t| glob.matches(t));
    }
    if let Some(min) = &project.min_tag {
        out.retain(|t| version_cmp(t.as_str(), min.as_str()) != Ordering::Greater);
    }
    if !project.exclude_tag_markers.is_empty() {
        out.retain(|t| !project.is_prerelease_tag(t));
    }
    filter_tags_since(out, since)
}

fn build_snapshot(
    p: &Project,
    repo: &GitRepo,
    tag: &TagName,
) -> CliResult<Snapshot<state::Canonical>> {
    let commit = repo
        .resolve_tag(tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let (files, scanned_paths, apps_scanned) = match p.layout {
        ProjectLayout::SingleApp => {
            let scan_paths = p.scan_paths.clone();
            let blobs = repo
                .read_paths_at_commit(&commit, |path| matches_any(path, &scan_paths))
                .map_err(|e| CliError::Core(e.into()))?;
            let files: Vec<(PathBuf, Vec<u8>)> =
                blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
            (files, scan_paths, Vec::new())
        }
        ProjectLayout::MultiApp | ProjectLayout::ErlangOtp => {
            let apps_set: RefCell<BTreeSet<ApplicationName>> = RefCell::new(BTreeSet::new());
            let blobs = repo
                .read_paths_at_commit(&commit, |path| match multi_app_match(path, p) {
                    Some(app) => {
                        apps_set.borrow_mut().insert(app);
                        true
                    }
                    None => false,
                })
                .map_err(|e| CliError::Core(e.into()))?;
            let files: Vec<(PathBuf, Vec<u8>)> =
                blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
            let apps: Vec<ApplicationName> = apps_set.into_inner().into_iter().collect();
            (files, p.app_roots.clone(), apps)
        }
    };
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
        scanned_paths,
        apps_scanned,
        generated_by: format!("backhopper {}", env!("CARGO_PKG_VERSION")),
        generated_at: OffsetDateTime::now_utc(),
    };
    let (modules, headers) = extracted;
    let snapshot = Snapshot::from_extracted(header, modules, headers).into_canonical();
    Ok(snapshot)
}

/// Decide whether `path` should be included in a `multi_app` or `erlang_otp`
/// snapshot, and if so, which application owns it. Returns `None` to drop.
pub fn multi_app_match(path: &str, project: &Project) -> Option<ApplicationName> {
    if !(path.ends_with(".erl") || path.ends_with(".hrl")) {
        return None;
    }
    let (app, within) = classify_app_path(path, &project.app_roots)?;
    if project.exclude_apps.iter().any(|a| a == &app) {
        return None;
    }
    if !project.include_apps.is_empty() && !project.include_apps.iter().any(|a| a == &app) {
        return None;
    }
    for sub in &project.excluded_subdirs {
        if let Some(rest) = within.strip_prefix(sub.as_str())
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return None;
        }
    }
    Some(app)
}

/// Match `path` against `app_roots` and return `(app_name, path_within_app)`.
///
/// Two pattern shapes are supported:
///  * `<dir>/*`: a wildcard. The path must be `{root}/{app}/{within}`, where
///    the Erlang app name is `{app}`.
///  * `<dir>`: a literal directory. The path must be `{root}/{within}`, where
///    the Erlang app name is the last path segment of `{root}` (e.g.
///    `erts/preloaded` produces app `preloaded`).
fn classify_app_path<'a>(
    path: &'a str,
    app_roots: &[String],
) -> Option<(ApplicationName, &'a str)> {
    for pattern in app_roots {
        if let Some(stripped_root) = pattern.strip_suffix("/*") {
            if let Some(after_root) = path.strip_prefix(stripped_root)
                && let Some(after_sep) = after_root.strip_prefix('/')
                && let Some(slash) = after_sep.find('/')
                && let Ok(app) = ApplicationName::new(&after_sep[..slash])
            {
                return Some((app, &after_sep[slash + 1..]));
            }
        } else if let Some(after_root) = path.strip_prefix(pattern.as_str())
            && let Some(within) = after_root.strip_prefix('/')
        {
            let last = pattern.rsplit('/').next().unwrap_or(pattern);
            if let Ok(app) = ApplicationName::new(last) {
                return Some((app, within));
            }
        }
    }
    None
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

#[derive(Debug, Serialize)]
struct ShowModulePayload<'a> {
    header: &'a SnapshotHeader,
    module: Option<&'a Module>,
    found: bool,
}

fn show(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    module: Option<ModuleName>,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store
        .read(&project, &tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let ctx = OutputContext::new(args.formatter, "snapshots show");
    if let Some(name) = &module {
        let module_ref = snapshot.module_named(name);
        let found = module_ref.is_some();
        let payload = ShowModulePayload {
            header: snapshot.header(),
            module: module_ref,
            found,
        };
        let exit = if found { 0 } else { 1 };
        render_with_exit(&ctx, &payload, exit, |w| {
            let mut buf: Vec<u8> = Vec::new();
            format::write_module_filtered(&snapshot, name, &mut buf)
                .map_err(|e| CliError::Core(CoreError::Snapshot(e)))?;
            w.write_all(&buf)?;
            if !found {
                writeln!(w, "module {name} not present in {project} {tag}")?;
            }
            Ok(())
        })?;
        return Ok(exit);
    }
    render(&ctx, &snapshot, |w| {
        let text =
            format::to_string(&snapshot).map_err(|e| CliError::Core(CoreError::Snapshot(e)))?;
        write!(w, "{}", text)?;
        Ok(())
    })?;
    Ok(0)
}

fn verify(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    tag: Option<TagName>,
    all: bool,
) -> CliResult<i32> {
    if all {
        return verify_all(args, cfg);
    }
    let project = project.ok_or_else(|| {
        CliError::InvalidInput("--project is required unless --all is set".into())
    })?;
    let tag =
        tag.ok_or_else(|| CliError::InvalidInput("--tag is required unless --all is set".into()))?;
    verify_one(args, cfg, project, tag)
}

fn verify_one(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
) -> CliResult<i32> {
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

#[derive(Debug, Serialize)]
struct VerifyAllFailure {
    project: String,
    tag: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct VerifyAllPayload {
    verified: usize,
    failed: Vec<VerifyAllFailure>,
}

fn verify_all(args: &GlobalArgs, cfg: &Config) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let projects = store
        .list_projects()
        .map_err(|e| CliError::Core(e.into()))?;
    let mut verified = 0usize;
    let mut failed: Vec<VerifyAllFailure> = Vec::new();
    for project in projects {
        let tags = store
            .list_tags(&project)
            .map_err(|e| CliError::Core(e.into()))?;
        for tag in tags {
            match store.read(&project, &tag) {
                Ok(_) => verified += 1,
                Err(e) => failed.push(VerifyAllFailure {
                    project: project.to_string(),
                    tag: tag.to_string(),
                    reason: e.to_string(),
                }),
            }
        }
    }
    let payload = VerifyAllPayload { verified, failed };
    let exit = if payload.failed.is_empty() { 0 } else { 1 };
    let ctx = OutputContext::new(args.formatter, "snapshots verify");
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "verified: {}, failed: {}",
            payload.verified,
            payload.failed.len()
        )?;
        for f in &payload.failed {
            writeln!(w, "  {} {}: {}", f.project, f.tag, f.reason)?;
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

#[derive(Debug, Serialize)]
pub struct LookupAllTagsRow {
    pub mfa: String,
    pub first_tag: Option<String>,
    pub last_tag: Option<String>,
    pub tags_present: usize,
}

#[derive(Debug, Serialize)]
struct LookupAllTagsPayload {
    project: String,
    rows: Vec<LookupAllTagsRow>,
}

fn lookup(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: Option<TagName>,
    mfas: Vec<Mfa>,
    include_hidden: bool,
    all_tags: bool,
) -> CliResult<i32> {
    if all_tags {
        return lookup_all_tags(args, cfg, project, mfas, include_hidden);
    }
    let tag = tag.ok_or_else(|| {
        CliError::InvalidInput("--tag is required unless --all-tags is set".into())
    })?;
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

fn lookup_all_tags(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    mfas: Vec<Mfa>,
    include_hidden: bool,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let mut tags = store
        .list_tags(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    if tags.is_empty() {
        return Err(CliError::InvalidInput(format!(
            "no snapshots on disk for project {project}"
        )));
    }
    // list_tags is lexicographic: re-sort by version_cmp so iteration is oldest-first.
    tags.sort_by(|a, b| version_cmp(b.as_str(), a.as_str()));
    // parse each snapshot once and check every MFA against it in the same pass.
    let mut firsts: Vec<Option<String>> = vec![None; mfas.len()];
    let mut lasts: Vec<Option<String>> = vec![None; mfas.len()];
    let mut counts: Vec<usize> = vec![0; mfas.len()];
    for tag in &tags {
        let snapshot = store
            .read(&project, tag)
            .map_err(|e| CliError::Core(e.into()))?;
        for (i, mfa) in mfas.iter().enumerate() {
            let module = snapshot.module_named(&mfa.module);
            let allowed = match module {
                Some(m) => include_hidden || matches!(m.visibility, Visibility::Public),
                None => true,
            };
            let target = FunArity {
                name: mfa.function.clone(),
                arity: mfa.arity,
            };
            let present =
                allowed && module.is_some_and(|m| m.exports.binary_search(&target).is_ok());
            if present {
                if firsts[i].is_none() {
                    firsts[i] = Some(tag.to_string());
                }
                lasts[i] = Some(tag.to_string());
                counts[i] += 1;
            }
        }
    }
    let rows: Vec<LookupAllTagsRow> = mfas
        .iter()
        .enumerate()
        .map(|(i, mfa)| LookupAllTagsRow {
            mfa: mfa.to_string(),
            first_tag: firsts[i].clone(),
            last_tag: lasts[i].clone(),
            tags_present: counts[i],
        })
        .collect();
    let exit = if rows.iter().all(|r| r.tags_present > 0) {
        0
    } else {
        1
    };
    let payload = LookupAllTagsPayload {
        project: project.to_string(),
        rows,
    };
    let ctx = OutputContext::new(args.formatter, "snapshots lookup");
    render_with_exit(&ctx, &payload, exit, |w| {
        for r in &payload.rows {
            match (&r.first_tag, &r.last_tag) {
                (Some(f), Some(l)) if f == l => {
                    writeln!(w, "{}\tpresent at {} only", r.mfa, f)?;
                }
                (Some(f), Some(l)) => {
                    writeln!(
                        w,
                        "{}\tpresent {}..{}\t({} tags)",
                        r.mfa, f, l, r.tags_present
                    )?;
                }
                _ => {
                    writeln!(w, "{}\tnot present in any stored snapshot", r.mfa)?;
                }
            }
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

#[derive(Debug, Serialize)]
pub struct CrossSeriesDiffPayload {
    pub from_series: String,
    pub to_series: String,
    pub projects: Vec<DiffPayload>,
}

fn diff(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    from: Option<TagName>,
    to: Option<TagName>,
    from_series: Option<SeriesName>,
    to_series: Option<SeriesName>,
) -> CliResult<i32> {
    match (project, from, to, from_series, to_series) {
        (Some(project), Some(from), Some(to), None, None) => {
            diff_single_project(args, cfg, project, from, to)
        }
        (None, None, None, Some(from_series), Some(to_series)) => {
            diff_cross_series(args, cfg, from_series, to_series)
        }
        _ => Err(CliError::InvalidInput(
            "pass either --project --from --to or --from-series --to-series".into(),
        )),
    }
}

fn diff_single_project(
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

fn diff_cross_series(
    args: &GlobalArgs,
    cfg: &Config,
    from_series: SeriesName,
    to_series: SeriesName,
) -> CliResult<i32> {
    let from = cfg
        .series_by_name(&from_series)
        .map_err(|e| CliError::Core(e.into()))?;
    let to = cfg
        .series_by_name(&to_series)
        .map_err(|e| CliError::Core(e.into()))?;
    let store = open_store_read(args, cfg)?;
    let from_resolved = from
        .resolve_pins(&store)
        .map_err(|e| CliError::Core(e.into()))?;
    let to_resolved = to
        .resolve_pins(&store)
        .map_err(|e| CliError::Core(e.into()))?;
    let mut to_by_project: BTreeMap<&ProjectName, VecDeque<&TagName>> = BTreeMap::new();
    for pin in &to_resolved {
        to_by_project
            .entry(&pin.project)
            .or_default()
            .push_back(&pin.tag);
    }
    let mut projects: Vec<DiffPayload> = Vec::new();
    for pin in &from_resolved {
        let Some(to_tag) = to_by_project
            .get_mut(&pin.project)
            .and_then(|q| q.pop_front())
        else {
            continue;
        };
        let a = store
            .read(&pin.project, &pin.tag)
            .map_err(|e| CliError::Core(e.into()))?;
        let b = store
            .read(&pin.project, to_tag)
            .map_err(|e| CliError::Core(e.into()))?;
        projects.push(compute_diff(&a, &b));
    }
    let payload = CrossSeriesDiffPayload {
        from_series: from_series.to_string(),
        to_series: to_series.to_string(),
        projects,
    };
    let ctx = OutputContext::new(args.formatter, "snapshots diff");
    render(&ctx, &payload, |w| {
        let mut first = true;
        for d in &payload.projects {
            if !first {
                writeln!(w)?;
            }
            first = false;
            writeln!(w, "# project: {} ({} -> {})", d.project, d.from, d.to)?;
            render_diff_text(w, d).map_err(CliError::from)?;
        }
        Ok(())
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
