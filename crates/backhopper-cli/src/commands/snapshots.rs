// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use crate::outcome::CommandOutcome;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use clap::crate_version;
use serde::Serialize;
use time::OffsetDateTime;

use backhopper_core::Snapshot;
use backhopper_core::config::{Config, Language, Project, ProjectLayout};
use backhopper_core::model::names::{
    ApplicationName, CommitSha, Mfa, ModuleName, ProjectName, SeriesName, TagName,
};
use backhopper_core::model::pin::PinSpec;
use backhopper_core::model::snapshot::{
    FunArity, Module, SnapshotHeader, Visibility, WireConstantBinding, WireValue, state,
};
use backhopper_core::model::snapshot_diff::{
    CrossSeriesDiffPayload, DiffPayload, QualifiedFunArity, QualifiedRecord, QualifiedTypeArity,
    VersionedMachineVersionChange, WireConstantChange,
};
use backhopper_core::snapshot::format;
use backhopper_core::store::{Mutable, SnapshotStore};
use backhopper_core::versions::version_cmp;
use backhopper_elixir::ElixirExtractor;
use backhopper_erlang::ErlangExtractor;
use backhopper_git::GitRepo;

use crate::cli::{GlobalArgs, SnapshotsCmd};
use crate::commands::auto_generate::expected_extractor_version;
use crate::commands::context::{load_config, open_project_repo, open_store_mut, open_store_read};
use crate::commands::pin_coverage::{PinCoverage, classify_pin};
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

impl DiscoverPayload {
    fn skipped_one(project: &ProjectName) -> Self {
        Self {
            project: project.to_string(),
            captured: 0,
            skipped: 1,
            failed: Vec::new(),
            tags: Vec::new(),
            ignored_non_tag_refs: Vec::new(),
        }
    }

    fn captured_one(project: &ProjectName, tag: &TagName) -> Self {
        Self {
            project: project.to_string(),
            captured: 1,
            skipped: 0,
            failed: Vec::new(),
            tags: vec![tag.to_string()],
            ignored_non_tag_refs: Vec::new(),
        }
    }

    fn single_failure(project: &ProjectName, tag: &TagName, reason: String) -> Self {
        Self {
            project: project.to_string(),
            captured: 0,
            skipped: 0,
            failed: vec![DiscoverFailure {
                tag: tag.to_string(),
                reason,
            }],
            tags: Vec::new(),
            ignored_non_tag_refs: Vec::new(),
        }
    }
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

// snapshots diff payloads are defined in backhopper_core::model::snapshot_diff so the CLI and the driver share one definition

pub fn handle(args: &GlobalArgs, cmd: SnapshotsCmd) -> CliResult<CommandOutcome> {
    let cfg = load_config(args)?;
    match cmd {
        SnapshotsCmd::ListTags { project } => list_tags(args, &cfg, project),
        SnapshotsCmd::Generate {
            project,
            no_remote_check: _,
            dry_run,
            since,
            series,
        } => generate(args, &cfg, project, dry_run, since, series),
        SnapshotsCmd::List { project } => list(args, &cfg, project),
        SnapshotsCmd::Show { pt, module } => show(args, &cfg, pt.project, pt.tag, module),
        SnapshotsCmd::Verify {
            project,
            tag,
            all,
            coverage,
        } => verify(args, &cfg, project, tag, all, coverage),
        SnapshotsCmd::Rebuild { pt, dry_run } => rebuild(args, &cfg, pt.project, pt.tag, dry_run),
        SnapshotsCmd::Migrate { project, dry_run } => migrate(args, &cfg, project, dry_run),
        SnapshotsCmd::Lookup { pt, query } => lookup(
            args,
            &cfg,
            pt.project,
            pt.tag,
            query.mfa,
            query.include_hidden,
        ),
        SnapshotsCmd::Introduced {
            project,
            query,
            timeline,
        } => introduced(
            args,
            &cfg,
            project,
            query.mfa,
            query.include_hidden,
            timeline,
        ),
        SnapshotsCmd::Modules { pt, include_hidden } => {
            modules(args, &cfg, pt.project, pt.tag, include_hidden)
        }
        SnapshotsCmd::Exports { pt, module } => exports(args, &cfg, pt.project, pt.tag, module),
        SnapshotsCmd::ProjectDiff { project, from, to } => {
            diff_single_project(args, &cfg, project, from, to)
        }
        SnapshotsCmd::SeriesDiff {
            from_series,
            to_series,
        } => diff_cross_series(args, &cfg, from_series, to_series),
    }
}

fn list_tags(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
) -> CliResult<CommandOutcome> {
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p)?],
        None => cfg.projects.iter().collect(),
    };
    let store = open_store_read(args, cfg)?;
    let mut payloads: Vec<ListTagsEntry> = Vec::new();
    for p in projects {
        let repo = open_project_repo(p)?;
        let listing = repo.list_tag_refs()?;
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
                writeln!(w, "  {tag}")?;
            }
        }
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn generate(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    dry_run: bool,
    since: Option<TagName>,
    series: Option<SeriesName>,
) -> CliResult<CommandOutcome> {
    if let Some(name) = series {
        return generate_series(args, cfg, &name, dry_run);
    }
    let store = open_store_mut(args, cfg)?;
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p)?],
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
    Ok(CommandOutcome::Success)
}

fn generate_one(
    p: &Project,
    store: &SnapshotStore<Mutable>,
    dry_run: bool,
    since: Option<&TagName>,
) -> CliResult<DiscoverPayload> {
    let repo = open_project_repo(p)?;
    let listing = repo.list_tag_refs()?;
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

#[derive(Debug, Serialize)]
struct GenerateSeriesPayload {
    series: String,
    projects: Vec<DiscoverPayload>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    self_pins_skipped: Vec<SelfPinSkip>,
    summary: GenerateSeriesSummary,
}

#[derive(Debug, Serialize)]
struct SelfPinSkip {
    project: String,
    git_ref: String,
    reason: &'static str,
}

#[derive(Debug, Serialize, Default)]
struct GenerateSeriesSummary {
    discovered: usize,
    skipped: usize,
    failed: usize,
}

fn generate_series(
    args: &GlobalArgs,
    cfg: &Config,
    name: &SeriesName,
    dry_run: bool,
) -> CliResult<CommandOutcome> {
    let series = cfg.series_by_name(name)?;
    let store = open_store_mut(args, cfg)?;
    let mut payloads: Vec<DiscoverPayload> = Vec::with_capacity(series.pins.len());
    let mut self_pins_skipped: Vec<SelfPinSkip> = Vec::new();
    let mut summary = GenerateSeriesSummary::default();
    for spec in &series.pins {
        match spec {
            PinSpec::SelfRef {
                project, git_ref, ..
            } => {
                self_pins_skipped.push(SelfPinSkip {
                    project: project.to_string(),
                    git_ref: git_ref.to_string(),
                    reason: "self snapshots are sourced from the working repo, not the store",
                });
            }
            PinSpec::Literal { project, tag } => {
                let p = cfg.project(project)?;
                let payload = generate_pinned_tag(p, tag, &store, dry_run)?;
                accumulate_summary(&mut summary, &payload);
                payloads.push(payload);
            }
            PinSpec::Pattern { project, .. } => {
                let p = cfg.project(project)?;
                let resolved = spec.resolve(&store)?;
                let payload = generate_pinned_tag(p, &resolved.tag, &store, dry_run)?;
                accumulate_summary(&mut summary, &payload);
                payloads.push(payload);
            }
        }
    }
    let payload = GenerateSeriesPayload {
        series: name.to_string(),
        projects: payloads,
        self_pins_skipped,
        summary,
    };
    let exit = CommandOutcome::from_success(payload.summary.failed == 0);
    let ctx = OutputContext::new(args.formatter, "snapshots generate");
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "series {}: discovered {}, skipped {}, failed {}",
            payload.series,
            payload.summary.discovered,
            payload.summary.skipped,
            payload.summary.failed
        )?;
        for p in &payload.projects {
            writeln!(
                w,
                "  {}: captured {}, skipped {}, failed {}",
                p.project,
                p.captured,
                p.skipped,
                p.failed.len()
            )?;
        }
        for s in &payload.self_pins_skipped {
            writeln!(w, "  {} ({}): skipped: {}", s.project, s.git_ref, s.reason)?;
        }
        Ok(())
    })?;
    Ok(exit)
}

fn accumulate_summary(summary: &mut GenerateSeriesSummary, payload: &DiscoverPayload) {
    summary.discovered += payload.captured;
    summary.skipped += payload.skipped;
    summary.failed += payload.failed.len();
}

fn generate_pinned_tag(
    p: &Project,
    tag: &TagName,
    store: &SnapshotStore<Mutable>,
    dry_run: bool,
) -> CliResult<DiscoverPayload> {
    if store.has(&p.name, tag) {
        return Ok(DiscoverPayload::skipped_one(&p.name));
    }
    let repo = open_project_repo(p)?;
    match build_snapshot(p, &repo, tag) {
        Ok(snapshot) => {
            if !dry_run {
                if let Err(e) = store.write(&snapshot) {
                    return Ok(DiscoverPayload::single_failure(&p.name, tag, e.to_string()));
                }
            }
            Ok(DiscoverPayload::captured_one(&p.name, tag))
        }
        Err(e) => Ok(DiscoverPayload::single_failure(&p.name, tag, e.to_string())),
    }
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

pub(crate) fn build_snapshot(
    p: &Project,
    repo: &GitRepo,
    tag: &TagName,
) -> CliResult<Snapshot<state::Canonical>> {
    let commit = repo.resolve_tag(tag)?;
    build_snapshot_at_commit(p, repo, &commit, tag)
}

pub(crate) fn build_snapshot_at_commit(
    p: &Project,
    repo: &GitRepo,
    commit: &CommitSha,
    tag: &TagName,
) -> CliResult<Snapshot<state::Canonical>> {
    let commit = commit.clone();
    let (files, scanned_paths, apps_scanned) = match p.layout {
        ProjectLayout::SingleApp => {
            let scan_paths = p.scan_paths.clone();
            let blobs =
                repo.read_paths_at_commit(&commit, |path| matches_any(path, &scan_paths))?;
            let files: Vec<(PathBuf, Vec<u8>)> =
                blobs.into_iter().map(|b| (b.path, b.bytes)).collect();
            (files, scan_paths, Vec::new())
        }
        ProjectLayout::MultiApp | ProjectLayout::ErlangOtp => {
            let apps_set: RefCell<BTreeSet<ApplicationName>> = RefCell::new(BTreeSet::new());
            let blobs =
                repo.read_paths_at_commit(&commit, |path| match multi_app_match(path, p) {
                    Some(app) => {
                        apps_set.borrow_mut().insert(app);
                        true
                    }
                    None => false,
                })?;
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
        generated_by: format!("backhopper {}", crate_version!()),
        generated_at: OffsetDateTime::now_utc(),
        extractor_version: match p.language {
            Language::Erlang => backhopper_erlang::EXTRACTOR_VERSION.to_owned(),
            Language::Elixir => backhopper_elixir::EXTRACTOR_VERSION.to_owned(),
        },
        dep_pins: Vec::new(),
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

fn list(args: &GlobalArgs, cfg: &Config, project: ProjectName) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let tags = store.list_tags(&project)?;
    let payload = ListPayload {
        project: project.to_string(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
    };
    let ctx = OutputContext::new(args.formatter, "snapshots list");
    render(&ctx, &payload, |w| {
        for t in &payload.tags {
            writeln!(w, "{t}")?;
        }
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
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
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store.read(&project, &tag)?;
    let ctx = OutputContext::new(args.formatter, "snapshots show");
    if let Some(name) = &module {
        let module_ref = snapshot.module_named(name);
        let found = module_ref.is_some();
        let payload = ShowModulePayload {
            header: snapshot.header(),
            module: module_ref,
            found,
        };
        let exit = CommandOutcome::from_success(found);
        render_with_exit(&ctx, &payload, exit, |w| {
            let mut buf: Vec<u8> = Vec::new();
            format::write_module_filtered(&snapshot, name, &mut buf)?;
            w.write_all(&buf)?;
            if !found {
                writeln!(w, "module {name} not present in {project} {tag}")?;
            }
            Ok(())
        })?;
        return Ok(exit);
    }
    render(&ctx, &snapshot, |w| {
        let text = format::to_string(&snapshot)?;
        write!(w, "{text}")?;
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn verify(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    tag: Option<TagName>,
    all: bool,
    coverage: bool,
) -> CliResult<CommandOutcome> {
    if coverage {
        return verify_coverage(args, cfg);
    }
    if all {
        return verify_all(args, cfg);
    }
    let project = project.ok_or_else(|| {
        CliError::InvalidInput("--project is required unless --all or --coverage is set".into())
    })?;
    let tag = tag.ok_or_else(|| {
        CliError::InvalidInput("--tag is required unless --all or --coverage is set".into())
    })?;
    verify_one(args, cfg, project, tag)
}

/// A module clone with `clause_heads` dropped: the text writer cannot
/// round-trip them yet, so verify compares modules without them.
fn without_clause_heads(m: &Module) -> Module {
    let mut m = m.clone();
    m.clause_heads.clear();
    m
}

fn verify_one(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
) -> CliResult<CommandOutcome> {
    let p = cfg.project(&project)?;
    let repo = open_project_repo(p)?;
    let regenerated = build_snapshot(p, &repo, &tag)?;
    let store = open_store_read(args, cfg)?;
    let stored = store.read(&project, &tag)?;
    // The text writer can't round-trip Module.clause_heads yet, so verify ignores them.
    let regenerated_modules: Vec<Module> = regenerated
        .modules()
        .iter()
        .map(without_clause_heads)
        .collect();
    let stored_modules: Vec<Module> = stored.modules().iter().map(without_clause_heads).collect();
    let matches = regenerated_modules == stored_modules
        && regenerated.headers() == stored.headers()
        && regenerated.header().commit == stored.header().commit;
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "matches": matches,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots verify");
    let exit = CommandOutcome::from_success(matches);
    render_with_exit(&ctx, &payload, exit, |w| {
        if matches {
            writeln!(w, "ok: {project} {tag}")?;
        } else {
            writeln!(w, "drift: {project} {tag}")?;
        }
        Ok(())
    })
}

/// One project-tag failure row, shared by `verify --all` and `migrate`,
/// whose JSON is identical.
#[derive(Debug, Serialize)]
struct TaggedFailure {
    project: String,
    tag: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct VerifyAllStale {
    project: String,
    tag: String,
    stored: String,
    expected: String,
}

#[derive(Debug, Serialize)]
struct VerifyAllPayload {
    verified: usize,
    failed: Vec<TaggedFailure>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stale_extractor: Vec<VerifyAllStale>,
}

fn verify_all(args: &GlobalArgs, cfg: &Config) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let projects = store.list_projects()?;
    let mut verified = 0usize;
    let mut failed: Vec<TaggedFailure> = Vec::new();
    let mut stale_extractor: Vec<VerifyAllStale> = Vec::new();
    for project in projects {
        let tags = store.list_tags(&project)?;
        let expected = expected_extractor_version(cfg, &project);
        for tag in tags {
            match store.read(&project, &tag) {
                Ok(snap) => {
                    verified += 1;
                    let stored = snap.header().extractor_version.as_str();
                    if !stored.is_empty() && stored != expected {
                        stale_extractor.push(VerifyAllStale {
                            project: project.to_string(),
                            tag: tag.to_string(),
                            stored: stored.to_owned(),
                            expected: expected.to_owned(),
                        });
                    }
                }
                Err(e) => failed.push(TaggedFailure {
                    project: project.to_string(),
                    tag: tag.to_string(),
                    reason: e.to_string(),
                }),
            }
        }
    }
    let payload = VerifyAllPayload {
        verified,
        failed,
        stale_extractor,
    };
    let exit = verify_all_exit_code(
        payload.verified,
        payload.failed.len(),
        payload.stale_extractor.len(),
    )?;
    let ctx = OutputContext::new(args.formatter, "snapshots verify");
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "verified: {}, failed: {}, stale_extractor: {}",
            payload.verified,
            payload.failed.len(),
            payload.stale_extractor.len(),
        )?;
        for f in &payload.failed {
            writeln!(w, "  fail  {} {}: {}", f.project, f.tag, f.reason)?;
        }
        for s in &payload.stale_extractor {
            writeln!(
                w,
                "  stale {} {} (extractor {} != binary {}): run `snapshots rebuild`",
                s.project, s.tag, s.stored, s.expected,
            )?;
        }
        Ok(())
    })
}

#[derive(Debug, Serialize)]
struct MissingPin {
    series: String,
    project: String,
    tag: String,
}

#[derive(Debug, Serialize)]
struct CoveragePayload {
    series_checked: usize,
    pins_checked: usize,
    covered: usize,
    self_pins_skipped: usize,
    missing_pins: Vec<MissingPin>,
}

fn verify_coverage(args: &GlobalArgs, cfg: &Config) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let mut missing_pins: Vec<MissingPin> = Vec::new();
    let mut pins_checked = 0usize;
    let mut covered = 0usize;
    let mut self_pins_skipped = 0usize;
    for series in &cfg.series {
        for spec in &series.pins {
            pins_checked += 1;
            match classify_pin(spec, &store) {
                PinCoverage::SelfPin => self_pins_skipped += 1,
                PinCoverage::Resolved { present: true, .. } => covered += 1,
                PinCoverage::Resolved {
                    pin,
                    present: false,
                } => {
                    missing_pins.push(MissingPin {
                        series: series.name.to_string(),
                        project: pin.project.to_string(),
                        tag: pin.tag.to_string(),
                    });
                }
                PinCoverage::Unresolved { project } => {
                    missing_pins.push(MissingPin {
                        series: series.name.to_string(),
                        project: project.to_string(),
                        tag: "<pattern unresolved>".to_string(),
                    });
                }
            }
        }
    }
    let payload = CoveragePayload {
        series_checked: cfg.series.len(),
        pins_checked,
        covered,
        self_pins_skipped,
        missing_pins,
    };
    let exit = CommandOutcome::from_success(payload.missing_pins.is_empty());
    let ctx = OutputContext::new(args.formatter, "snapshots verify");
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "series: {}, pins: {}, covered: {}, missing: {}, self_pins_skipped: {}",
            payload.series_checked,
            payload.pins_checked,
            payload.covered,
            payload.missing_pins.len(),
            payload.self_pins_skipped,
        )?;
        for m in &payload.missing_pins {
            writeln!(w, "  missing {} {} ({})", m.project, m.tag, m.series)?;
        }
        Ok(())
    })
}

// 0 all loaded and current; 3 partial or stale; Err when zero loaded
pub fn verify_all_exit_code(
    verified: usize,
    failed: usize,
    stale: usize,
) -> CliResult<CommandOutcome> {
    if failed > 0 && verified == 0 {
        return Err(CliError::Other(format!(
            "snapshots verify --all: every snapshot failed to load ({failed} failure(s))"
        )));
    }
    if failed > 0 || stale > 0 {
        return Ok(CommandOutcome::PartialSuccess);
    }
    Ok(CommandOutcome::Success)
}

#[derive(Debug, Serialize)]
struct MigratePayload {
    migrated: usize,
    already_current: usize,
    failed: Vec<TaggedFailure>,
}

fn migrate(
    args: &GlobalArgs,
    cfg: &Config,
    project_filter: Option<ProjectName>,
    dry_run: bool,
) -> CliResult<CommandOutcome> {
    let store_read = open_store_read(args, cfg)?;
    let store_mut = if dry_run {
        None
    } else {
        Some(open_store_mut(args, cfg)?)
    };
    let projects = match project_filter {
        Some(p) => vec![p],
        None => store_read.list_projects()?,
    };
    let mut migrated = 0usize;
    let mut already_current = 0usize;
    let mut failed: Vec<TaggedFailure> = Vec::new();
    for project in projects {
        let tags = store_read.list_tags(&project)?;
        for tag in tags {
            match store_read.read(&project, &tag) {
                Ok(snap) => {
                    let stored_path = store_read.snapshot_path(&project, &tag)?;
                    let current_text = fs::read_to_string(&stored_path).unwrap_or_default();
                    let re_emitted = backhopper_core::snapshot::format::to_string(&snap)?;
                    if current_text == re_emitted {
                        already_current += 1;
                        continue;
                    }
                    if let Some(mw) = &store_mut {
                        mw.write(&snap)?;
                    }
                    migrated += 1;
                }
                Err(e) => failed.push(TaggedFailure {
                    project: project.to_string(),
                    tag: tag.to_string(),
                    reason: e.to_string(),
                }),
            }
        }
    }
    let payload = MigratePayload {
        migrated,
        already_current,
        failed,
    };
    let exit = CommandOutcome::from_success(payload.failed.is_empty());
    let ctx = OutputContext::new(args.formatter, "snapshots migrate");
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "migrated: {}, already_current: {}, failed: {}",
            payload.migrated,
            payload.already_current,
            payload.failed.len()
        )?;
        for f in &payload.failed {
            writeln!(w, "  fail  {} {}: {}", f.project, f.tag, f.reason)?;
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
) -> CliResult<CommandOutcome> {
    let p = cfg.project(&project)?;
    let repo = open_project_repo(p)?;
    let snapshot = build_snapshot(p, &repo, &tag)?;
    let store = open_store_mut(args, cfg)?;
    if !dry_run {
        let _ = store.delete(&project, &tag);
        store.write(&snapshot)?;
    }
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "rebuilt": !dry_run,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots rebuild");
    render(&ctx, &payload, |w| {
        writeln!(w, "rebuilt {project} {tag}")?;
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn lookup(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    mfas: Vec<Mfa>,
    include_hidden: bool,
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store.read(&project, &tag)?;
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
    let exit = CommandOutcome::from_success(all_found);
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

#[derive(Debug, Clone)]
pub struct TagSnapshot {
    pub tag: TagName,
    pub commit: CommitSha,
    pub presence: Vec<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub tag: TagName,
    pub commit: CommitSha,
    pub present: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct IntroducedRow {
    pub mfa: String,
    pub first_tag: Option<TagName>,
    pub first_commit: Option<CommitSha>,
    pub last_tag: Option<TagName>,
    pub last_commit: Option<CommitSha>,
    pub tags_present: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline: Option<Vec<TimelineEntry>>,
}

#[derive(Debug, Serialize)]
struct IntroducedPayload {
    project: String,
    rows: Vec<IntroducedRow>,
}

fn introduced(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    mfas: Vec<Mfa>,
    include_hidden: bool,
    timeline: bool,
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let walk = walk_project_snapshots(&store, &project, &mfas, include_hidden)?;
    let rows = compute_introduced_rows(&walk, &mfas, timeline);
    let exit = CommandOutcome::from_success(rows.iter().all(|r| r.tags_present > 0));
    let payload = IntroducedPayload {
        project: project.to_string(),
        rows,
    };
    let ctx = OutputContext::new(args.formatter, "snapshots introduced");
    render_with_exit(&ctx, &payload, exit, |w| {
        render_introduced_text(&payload, w)
    })
}

fn walk_project_snapshots<M>(
    store: &SnapshotStore<M>,
    project: &ProjectName,
    mfas: &[Mfa],
    include_hidden: bool,
) -> CliResult<Vec<TagSnapshot>> {
    let mut tags = store.list_tags(project)?;
    if tags.is_empty() {
        return Err(CliError::InvalidInput(format!(
            "no snapshots on disk for project {project}"
        )));
    }
    // version_cmp ranks newer versions first: swap arguments so iteration is oldest-first.
    tags.sort_by(|a, b| version_cmp(b.as_str(), a.as_str()));
    let mut out = Vec::with_capacity(tags.len());
    for tag in tags {
        let snapshot = store.read(project, &tag)?;
        let commit = snapshot.header().commit.clone();
        let presence: Vec<bool> = mfas
            .iter()
            .map(|mfa| mfa_present_in(&snapshot, mfa, include_hidden))
            .collect();
        out.push(TagSnapshot {
            tag,
            commit,
            presence,
        });
    }
    Ok(out)
}

fn mfa_present_in(snapshot: &Snapshot<state::Canonical>, mfa: &Mfa, include_hidden: bool) -> bool {
    let Some(module) = snapshot.module_named(&mfa.module) else {
        return false;
    };
    if !include_hidden && !matches!(module.visibility, Visibility::Public) {
        return false;
    }
    let target = FunArity {
        name: mfa.function.clone(),
        arity: mfa.arity,
    };
    module.exports.binary_search(&target).is_ok()
}

pub fn compute_introduced_rows(
    walk: &[TagSnapshot],
    mfas: &[Mfa],
    timeline: bool,
) -> Vec<IntroducedRow> {
    mfas.iter()
        .enumerate()
        .map(|(i, mfa)| {
            let mut first_idx: Option<usize> = None;
            let mut last_idx: Option<usize> = None;
            let mut count = 0_usize;
            for (idx, ts) in walk.iter().enumerate() {
                if ts.presence[i] {
                    first_idx.get_or_insert(idx);
                    last_idx = Some(idx);
                    count += 1;
                }
            }
            let endpoint = |idx: usize| (walk[idx].tag.clone(), walk[idx].commit.clone());
            let (first_tag, first_commit) = first_idx.map(endpoint).unzip();
            let (last_tag, last_commit) = last_idx.map(endpoint).unzip();
            let timeline = timeline.then(|| {
                walk.iter()
                    .map(|ts| TimelineEntry {
                        tag: ts.tag.clone(),
                        commit: ts.commit.clone(),
                        present: ts.presence[i],
                    })
                    .collect()
            });
            IntroducedRow {
                mfa: mfa.to_string(),
                first_tag,
                first_commit,
                last_tag,
                last_commit,
                tags_present: count,
                timeline,
            }
        })
        .collect()
}

fn render_introduced_text(payload: &IntroducedPayload, w: &mut dyn Write) -> CliResult<()> {
    for row in &payload.rows {
        match (&row.first_tag, &row.last_tag) {
            (Some(f), Some(l)) if f == l => writeln!(
                w,
                "{}\tpresent only at {} ({})",
                row.mfa,
                f,
                short_sha(row.first_commit.as_ref())
            )?,
            (Some(f), Some(l)) => writeln!(
                w,
                "{}\tfirst {} ({})\tlast {} ({})\tpresent in {} tags",
                row.mfa,
                f,
                short_sha(row.first_commit.as_ref()),
                l,
                short_sha(row.last_commit.as_ref()),
                row.tags_present,
            )?,
            _ => writeln!(w, "{}\tnot present in any stored snapshot", row.mfa)?,
        }
        if let Some(entries) = &row.timeline {
            for entry in entries {
                writeln!(
                    w,
                    "  {}\t{}\t{}",
                    entry.tag,
                    short_sha(Some(&entry.commit)),
                    if entry.present { "+" } else { "-" },
                )?;
            }
        }
    }
    Ok(())
}

fn short_sha(sha: Option<&CommitSha>) -> &str {
    match sha {
        Some(s) => &s.as_str()[..7.min(s.as_str().len())],
        None => "-",
    }
}

fn modules(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    include_hidden: bool,
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store.read(&project, &tag)?;
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
            writeln!(w, "{h}\theader")?;
        }
        Ok(())
    })?;
    Ok(CommandOutcome::Success)
}

fn exports(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    tag: TagName,
    module: ModuleName,
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let snapshot = store.read(&project, &tag)?;
    let m: Option<&Module> = snapshot.module_named(&module);
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
        "module":  module.to_string(),
        "exports": exports,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots exports");
    render(&ctx, &payload, |w| {
        for e in &exports {
            writeln!(w, "{e}")?;
        }
        Ok(())
    })?;
    Ok(CommandOutcome::from_success(m.is_some()))
}

fn diff_single_project(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    from: TagName,
    to: TagName,
) -> CliResult<CommandOutcome> {
    let store = open_store_read(args, cfg)?;
    let a = store.read(&project, &from)?;
    let b = store.read(&project, &to)?;
    let result = compute_diff(&a, &b);
    let ctx = OutputContext::new(args.formatter, "snapshots project_diff");
    render(&ctx, &result, |w| {
        render_diff_text(w, &result).map_err(CliError::from)
    })?;
    Ok(CommandOutcome::Success)
}

fn diff_cross_series(
    args: &GlobalArgs,
    cfg: &Config,
    from_series: SeriesName,
    to_series: SeriesName,
) -> CliResult<CommandOutcome> {
    let from = cfg.series_by_name(&from_series)?;
    let to = cfg.series_by_name(&to_series)?;
    let store = open_store_read(args, cfg)?;
    let from_resolved = from.resolve_pins(&store)?;
    let to_resolved = to.resolve_pins(&store)?;
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
            .and_then(|v| v.pop_front())
        else {
            continue;
        };
        let a = store.read(&pin.project, &pin.tag)?;
        let b = store.read(&pin.project, to_tag)?;
        projects.push(compute_diff(&a, &b));
    }
    let payload = CrossSeriesDiffPayload {
        from_series,
        to_series,
        projects,
    };
    let ctx = OutputContext::new(args.formatter, "snapshots series_diff");
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
    Ok(CommandOutcome::Success)
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
    for c in &d.versioned_machine_version_changes {
        match c {
            VersionedMachineVersionChange::Missing { module, side } => {
                writeln!(w, "versioned_machine_version missing on {side} {module}")?;
            }
            VersionedMachineVersionChange::Drift { module, from, to } => {
                writeln!(
                    w,
                    "versioned_machine_version drift {module} {} -> {}",
                    fmt_opt_u64(*from),
                    fmt_opt_u64(*to),
                )?;
            }
        }
    }
    for c in &d.wire_constant_changes {
        match c {
            WireConstantChange::Missing {
                module,
                side,
                macros,
            } => {
                writeln!(
                    w,
                    "wire_constant missing on {side} {module} [{}]",
                    macros.join(", "),
                )?;
            }
            WireConstantChange::Drift {
                module,
                macro_name,
                from,
                to,
            } => {
                writeln!(
                    w,
                    "wire_constant drift {module}.?{macro_name} {from} -> {to}",
                )?;
            }
        }
    }
    Ok(())
}

fn fmt_opt_u64(v: Option<u64>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "-".into(),
    }
}

pub fn compute_diff(a: &Snapshot<state::Canonical>, b: &Snapshot<state::Canonical>) -> DiffPayload {
    let a_names: BTreeSet<&ModuleName> = a.modules().iter().map(|m| &m.name).collect();
    let b_names: BTreeSet<&ModuleName> = b.modules().iter().map(|m| &m.name).collect();
    let modules_added: Vec<ModuleName> =
        b_names.difference(&a_names).map(|s| (*s).clone()).collect();
    let modules_removed: Vec<ModuleName> =
        a_names.difference(&b_names).map(|s| (*s).clone()).collect();
    let mut exports_added = Vec::new();
    let mut exports_removed = Vec::new();
    let mut types_added = Vec::new();
    let mut types_removed = Vec::new();
    let mut callbacks_added = Vec::new();
    let mut callbacks_removed = Vec::new();
    for name in a_names.union(&b_names) {
        diff_named_set(
            module_exports(a, name),
            module_exports(b, name),
            &mut exports_added,
            &mut exports_removed,
            |fa| QualifiedFunArity {
                module: (*name).clone(),
                fun_arity: fa,
            },
        );
        diff_named_set(
            module_callbacks(a, name),
            module_callbacks(b, name),
            &mut callbacks_added,
            &mut callbacks_removed,
            |fa| QualifiedFunArity {
                module: (*name).clone(),
                fun_arity: fa,
            },
        );
        diff_named_set(
            module_types(a, name),
            module_types(b, name),
            &mut types_added,
            &mut types_removed,
            |ta| QualifiedTypeArity {
                module: (*name).clone(),
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
            hrl_records(a, path),
            hrl_records(b, path),
            &mut records_added,
            &mut records_removed,
            |record| QualifiedRecord {
                header: (*path).to_owned(),
                record,
            },
        );
    }
    let mut versioned_machine_version_changes = Vec::new();
    let mut wire_constant_changes = Vec::new();
    for name in a_names.union(&b_names) {
        diff_versioned_machine_version(a, b, name, &mut versioned_machine_version_changes);
        diff_wire_constants(a, b, name, &mut wire_constant_changes);
    }
    DiffPayload {
        project: a.header().project.clone(),
        from: a.header().tag.clone(),
        to: b.header().tag.clone(),
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
        versioned_machine_version_changes,
        wire_constant_changes,
    }
}

fn diff_versioned_machine_version(
    a: &Snapshot<state::Canonical>,
    b: &Snapshot<state::Canonical>,
    name: &ModuleName,
    out: &mut Vec<VersionedMachineVersionChange>,
) {
    let from = a
        .module_named(name)
        .and_then(|m| m.versioned_machine_version.as_ref());
    let to = b
        .module_named(name)
        .and_then(|m| m.versioned_machine_version.as_ref());
    match (from, to) {
        (None, None) => {}
        (Some(_), None) => out.push(VersionedMachineVersionChange::Missing {
            module: name.clone(),
            side: "to".into(),
        }),
        (None, Some(_)) => out.push(VersionedMachineVersionChange::Missing {
            module: name.clone(),
            side: "from".into(),
        }),
        (Some(x), Some(y)) if x.value != y.value => {
            out.push(VersionedMachineVersionChange::Drift {
                module: name.clone(),
                from: x.value,
                to: y.value,
            });
        }
        _ => {}
    }
}

fn wire_macros_by_name<'a>(
    snap: &'a Snapshot<state::Canonical>,
    name: &ModuleName,
) -> BTreeMap<&'a str, &'a WireConstantBinding> {
    snap.module_named(name)
        .map(|m| {
            m.wire_constants
                .iter()
                .map(|wc| (wc.macro_name.as_str(), wc))
                .collect()
        })
        .unwrap_or_default()
}

fn diff_wire_constants(
    a: &Snapshot<state::Canonical>,
    b: &Snapshot<state::Canonical>,
    name: &ModuleName,
    out: &mut Vec<WireConstantChange>,
) {
    let from_map = wire_macros_by_name(a, name);
    let to_map = wire_macros_by_name(b, name);
    if from_map.is_empty() && to_map.is_empty() {
        return;
    }
    let mut missing_from: Vec<String> = Vec::new();
    let mut missing_to: Vec<String> = Vec::new();
    let keys: BTreeSet<&str> = from_map.keys().chain(to_map.keys()).copied().collect();
    for k in &keys {
        match (from_map.get(k), to_map.get(k)) {
            (Some(x), Some(y)) if x.value != y.value => {
                out.push(WireConstantChange::Drift {
                    module: name.clone(),
                    macro_name: (*k).to_owned(),
                    from: wire_value_text(&x.value),
                    to: wire_value_text(&y.value),
                });
            }
            (Some(_), None) => missing_to.push((*k).to_owned()),
            (None, Some(_)) => missing_from.push((*k).to_owned()),
            _ => {}
        }
    }
    missing_from.sort();
    missing_to.sort();
    if !missing_from.is_empty() {
        out.push(WireConstantChange::Missing {
            module: name.clone(),
            side: "from".into(),
            macros: missing_from,
        });
    }
    if !missing_to.is_empty() {
        out.push(WireConstantChange::Missing {
            module: name.clone(),
            side: "to".into(),
            macros: missing_to,
        });
    }
}

fn wire_value_text(v: &WireValue) -> String {
    match v {
        WireValue::U64(n) => format!("u64 {n}"),
        WireValue::Bytes(b) => format!("bytes {}", b.len()),
        WireValue::Opaque(s) => format!("opaque {s}"),
    }
}

fn diff_named_set<T, F>(
    a: BTreeSet<String>,
    b: BTreeSet<String>,
    added: &mut Vec<T>,
    removed: &mut Vec<T>,
    qualify: F,
) where
    F: Fn(String) -> T,
{
    for s in b.difference(&a) {
        added.push(qualify(s.clone()));
    }
    for s in a.difference(&b) {
        removed.push(qualify(s.clone()));
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
