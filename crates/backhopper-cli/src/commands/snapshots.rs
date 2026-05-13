use std::path::PathBuf;

use serde::Serialize;
use time::OffsetDateTime;

use backhopper_core::Error as CoreError;
use backhopper_core::config::{Config, Language, Project};
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, SnapshotHeader, state};
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

pub fn handle(args: &GlobalArgs, cmd: SnapshotsCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        SnapshotsCmd::Discover { project } => discover(args, &cfg, project, false),
        SnapshotsCmd::Update {
            project,
            no_remote_check: _,
        } => discover(args, &cfg, project, true),
        SnapshotsCmd::List { project } => list(args, &cfg, project),
        SnapshotsCmd::Show { project, tag } => show(args, &cfg, project, tag),
        SnapshotsCmd::Verify { project, tag } => verify(args, &cfg, project, tag),
        SnapshotsCmd::Rebuild { project, tag } => rebuild(args, &cfg, project, tag),
    }
}

fn discover(
    args: &GlobalArgs,
    cfg: &Config,
    project: Option<ProjectName>,
    update_only: bool,
) -> CliResult<i32> {
    let store = open_store_mut(args, cfg)?;
    let projects: Vec<&Project> = match project {
        Some(p) => vec![cfg.project(&p).map_err(|e| CliError::Core(e.into()))?],
        None => cfg.projects.iter().collect(),
    };
    let mut payloads = Vec::new();
    for p in projects {
        payloads.push(discover_one(args, p, &store, update_only)?);
    }
    let ctx = OutputContext::new(args.formatter, "snapshots discover");
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

fn discover_one(
    args: &GlobalArgs,
    p: &Project,
    store: &SnapshotStore<Mutable>,
    _update_only: bool,
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
                if !args.dry_run {
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
        path.ends_with(&format!(".{}", ext))
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

fn rebuild(args: &GlobalArgs, cfg: &Config, project: ProjectName, tag: TagName) -> CliResult<i32> {
    let p = cfg
        .project(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    let repo = GitRepo::open(p.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let snapshot = build_snapshot(p, &repo, &tag)?;
    let store = open_store_mut(args, cfg)?;
    if !args.dry_run {
        let _ = store.delete(&project, &tag);
        store
            .write(&snapshot)
            .map_err(|e| CliError::Core(e.into()))?;
    }
    let payload = serde_json::json!({
        "project": project.to_string(),
        "tag":     tag.to_string(),
        "rebuilt": !args.dry_run,
    });
    let ctx = OutputContext::new(args.formatter, "snapshots rebuild");
    render(&ctx, &payload, |w| {
        writeln!(w, "rebuilt {} {}", project, tag)?;
        Ok(())
    })?;
    Ok(0)
}
