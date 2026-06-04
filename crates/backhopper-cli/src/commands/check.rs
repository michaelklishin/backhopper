// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::mem;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str;

use bel7_cli::{
    InteractiveReporter, NonInteractiveReporter, ProgressReporter, QuietReporter, TableStyle,
};
use serde::Serialize;

use backhopper_core::Error as CoreError;
use backhopper_core::compat::is_otp_module;
use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::routing::{PathRouting, classify_paths_for_pin};
use backhopper_core::compat::scope::{PinScope, parse_module_names};
use backhopper_core::compat::source_macros::{FileMap, build_macro_table};
use backhopper_core::config::{Config, Project};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::errors::GitError;
use backhopper_core::git::{GitRepo, pr_commits_for};
use backhopper_core::model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::{self, Pin, PinSpec};
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::symbol::{SymbolKind, SymbolRef};
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PinVerdict, Reason, SeriesEvaluation, SeriesVerdict,
    TouchedKinds, TranslationSource, Verdict,
};
use backhopper_core::store::{ReadOnly, SnapshotStore};

use crate::cli::{CheckCmd, CheckFlags, Formatter, GlobalArgs, SourcePinArgs};
use crate::commands::auto_generate::{
    coverage_report, ensure_pin_snapshots_present, warn_on_stale_extractors,
};
use crate::commands::context::{load_config, open_store_read};
use crate::commands::self_snapshot::{ensure_self_snapshot_present, resolve_self_pin};
use crate::commands::snapshot_cache::SnapshotCache;
use crate::commands::suggest::{
    ProjectSuggestion, append_suggestions_to_config, build_suggestions, render_suggestion,
};
use crate::commands::target_repo;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_alts, render_with_exit};
use crate::tables::render_evaluation_table;

#[derive(Debug, Serialize)]
struct CompatPayload {
    queried_against: QueriedAgainst,
    results: SeriesVerdict,
    #[serde(skip_serializing_if = "Diagnostics::is_empty")]
    diagnostics: Diagnostics,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    project_suggestions: Vec<ProjectSuggestion>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum QueriedAgainst {
    Pin { project: String, tag: String },
    Series { name: String, pins: Vec<PinPayload> },
}

pub fn handle(args: &GlobalArgs, cmd: CheckCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    warn_on_deprecated_summary_only(&cmd);
    match cmd {
        CheckCmd::Patch {
            project,
            tag,
            series,
            repo_dir_path,
            source,
            target,
            diagnostics,
            patch_file_path,
        } => {
            let bytes = read_patch_input(patch_file_path)?;
            run_check_patch(
                args,
                &cfg,
                &bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                &FileMap::new(),
                &source,
                &target,
                diagnostics,
                None,
            )
        }
        CheckCmd::Commit {
            project,
            tag,
            series,
            repo_dir_path,
            source,
            target,
            diagnostics,
            commit,
        } => {
            let bytes = commit_patch_bytes(&repo_dir_path, &commit)?;
            let source_files = load_source_files_at_parent(&repo_dir_path, &commit)?;
            run_check_patch(
                args,
                &cfg,
                &bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                &source_files,
                &source,
                &target,
                diagnostics,
                None,
            )
        }
        CheckCmd::Range {
            project,
            tag,
            series,
            repo_dir_path,
            range,
            merge_commit,
            source,
            target,
            diagnostics,
        } => {
            let bytes =
                range_patch_bytes(&repo_dir_path, range.as_deref(), merge_commit.as_deref())?;
            let source_files =
                range_source_files(&repo_dir_path, range.as_deref(), merge_commit.as_deref())?;
            run_check_patch(
                args,
                &cfg,
                &bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                &source_files,
                &source,
                &target,
                diagnostics,
                None,
            )
        }
        CheckCmd::Merge {
            project,
            tag,
            series,
            repo_dir_path,
            source,
            target,
            diagnostics,
            merge_sha,
        } => {
            let bytes = range_patch_bytes(&repo_dir_path, None, Some(&merge_sha))?;
            let source_files = range_source_files(&repo_dir_path, None, Some(&merge_sha))?;
            run_check_patch(
                args,
                &cfg,
                &bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                &source_files,
                &source,
                &target,
                diagnostics,
                Some(&merge_sha),
            )
        }
        CheckCmd::Pr {
            project,
            tag,
            series,
            repo_dir_path,
            source,
            target,
            diagnostics,
            pr_url,
        } => {
            let bytes = pr_patch_bytes(&pr_url)?;
            run_check_patch(
                args,
                &cfg,
                &bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                &FileMap::new(),
                &source,
                &target,
                diagnostics,
                None,
            )
        }
        CheckCmd::Multi {
            series,
            repo_dir_path,
            source,
            target: _,
            diagnostics,
            commit,
        } => run_multi(
            args,
            &cfg,
            &series,
            &repo_dir_path,
            &commit,
            &source,
            diagnostics,
        ),
        CheckCmd::Batch {
            series,
            repo_dir_path,
            commits_file_path,
            source,
            target,
            diagnostics,
        } => run_batch(
            args,
            &cfg,
            &series,
            &repo_dir_path,
            &commits_file_path,
            &source,
            &target,
            diagnostics,
        ),
    }
}

fn lookup_commit_subject(sha_str: Option<&str>, repo: Option<&Path>) -> Option<String> {
    let sha_str = sha_str?;
    let repo_path = repo?;
    let sha = CommitSha::new(sha_str.to_owned()).ok()?;
    let repo = GitRepo::open(repo_path).ok()?;
    repo.commit_subject(&sha).ok()
}

fn warn_on_deprecated_summary_only(cmd: &CheckCmd) {
    let diagnostics = match cmd {
        CheckCmd::Patch { diagnostics, .. }
        | CheckCmd::Commit { diagnostics, .. }
        | CheckCmd::Range { diagnostics, .. }
        | CheckCmd::Merge { diagnostics, .. }
        | CheckCmd::Pr { diagnostics, .. }
        | CheckCmd::Multi { diagnostics, .. }
        | CheckCmd::Batch { diagnostics, .. } => diagnostics,
    };
    if diagnostics.summary_only {
        eprintln!(
            "backhopper: warning: --summary-only is deprecated and will be removed in 0.11.0; \
             use `--formatter summary` (JSONL) or `--formatter text-summary` (text) instead"
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PrCoordinates {
    pub owner: String,
    pub repo: String,
    pub number: u32,
}

pub fn parse_pr_url(url: &str) -> Result<PrCoordinates, String> {
    let stripped = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .ok_or_else(|| {
            format!("expected https://github.com/<owner>/<repo>/pull/<n>, got {url:?}")
        })?;
    let mut parts = stripped.split('/');
    let owner = parts
        .next()
        .ok_or_else(|| format!("missing owner in {url:?}"))?;
    let repo = parts
        .next()
        .ok_or_else(|| format!("missing repo in {url:?}"))?;
    let pull = parts
        .next()
        .ok_or_else(|| format!("missing 'pull' segment in {url:?}"))?;
    if pull != "pull" {
        return Err(format!("expected /pull/ segment in {url:?}, got {pull:?}"));
    }
    let number_str = parts
        .next()
        .ok_or_else(|| format!("missing PR number in {url:?}"))?;
    let number: u32 = number_str
        .parse()
        .map_err(|_| format!("PR number {number_str:?} is not a positive integer"))?;
    if owner.is_empty() || repo.is_empty() {
        return Err(format!("empty owner or repo in {url:?}"));
    }
    Ok(PrCoordinates {
        owner: owner.into(),
        repo: repo.into(),
        number,
    })
}

fn pr_patch_bytes(url: &str) -> CliResult<Vec<u8>> {
    let coords = parse_pr_url(url).map_err(CliError::InvalidInput)?;
    let output = Command::new("gh")
        .arg("pr")
        .arg("diff")
        .arg(coords.number.to_string())
        .arg("--repo")
        .arg(format!("{}/{}", coords.owner, coords.repo))
        .output()
        .map_err(|e| {
            CliError::InvalidInput(format!(
                "failed to invoke `gh pr diff`: {e} (is the `gh` CLI installed?)"
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CliError::InvalidInput(format!(
            "`gh pr diff {}` failed: {}",
            coords.number,
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

fn build_pin_files(
    project: &Project,
    pin: &Pin,
    scope: &PinScope,
    touched_paths: &[PathBuf],
    self_repo: Option<&Path>,
    pin_self_override: Option<&Path>,
) -> CliResult<EvaluationFiles> {
    let in_scope: Vec<(PathBuf, PathBuf)> = touched_paths
        .iter()
        .filter_map(|p| scope.rewrite_path(p).map(|r| (p.clone(), r.to_path_buf())))
        .collect();
    if in_scope.is_empty() {
        return Ok(EvaluationFiles::new());
    }
    let (repo, commit) = if project.is_self() {
        let repo_path = pin_self_override
            .or(self_repo)
            .ok_or_else(|| {
                CliError::InvalidInput(
                    "self-project evaluation requires `repo_dir_path` on the pin or `--repo-dir-path` on the CLI".into(),
                )
            })?
            .to_path_buf();
        // pin.tag for self-pins is the resolved commit SHA.
        let commit = CommitSha::new(pin.tag.as_str())
            .map_err(|e| CliError::Other(format!("self-pin tag is not a SHA: {e}")))?;
        let repo = GitRepo::open(repo_path).map_err(|e| CliError::Core(e.into()))?;
        (repo, commit)
    } else {
        let git_url = project
            .require_git_url()
            .map_err(|e| CliError::Core(e.into()))?
            .to_path_buf();
        let repo = GitRepo::open(git_url).map_err(|e| CliError::Core(e.into()))?;
        let commit = repo
            .resolve_tag(&pin.tag)
            .map_err(|e| CliError::Core(e.into()))?;
        (repo, commit)
    };
    let needed: BTreeSet<String> = in_scope
        .iter()
        .map(|(_, project_path)| project_path.to_string_lossy().into_owned())
        .collect();
    let blobs = repo
        .read_paths_at_commit(&commit, |p| needed.contains(p))
        .map_err(|e| CliError::Core(e.into()))?;
    let mut present: BTreeMap<String, Vec<u8>> = blobs
        .into_iter()
        .map(|b| (b.path.to_string_lossy().into_owned(), b.bytes))
        .collect();
    let mut files = EvaluationFiles::new();
    for (original_path, project_path) in &in_scope {
        let key = project_path.to_string_lossy().into_owned();
        let contents = present.remove(&key);
        files = files.with(original_path.clone(), contents);
    }
    Ok(files)
}

fn build_pin_scope(project: &Project, snapshot: &Snapshot<state::Canonical>) -> PinScope {
    let extra: Vec<_> = parse_module_names(
        project
            .public_modules
            .iter()
            .chain(project.internal_modules.iter()),
    );
    PinScope::from_snapshot(project.name.clone(), snapshot, extra)
}

fn read_patch_input(file: Option<PathBuf>) -> CliResult<Vec<u8>> {
    match file {
        Some(p) => Ok(fs::read(&p)?),
        None => {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf)?;
            if buf.is_empty() {
                return Err(CliError::InvalidInput(
                    "no patch on stdin and no --patch-file given".into(),
                ));
            }
            Ok(buf)
        }
    }
}

fn resolve_untracked_modules_against_tree(
    evaluation: &mut SeriesEvaluation,
    repo_dir: &Path,
) -> CliResult<()> {
    let modules_on_disk = scan_erl_modules(repo_dir)?;
    let missing: Vec<ModuleName> = evaluation
        .diagnostics
        .untracked_calls
        .keys()
        .filter(|m| !modules_on_disk.contains(m.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut updated_results: Vec<PinVerdict> = Vec::with_capacity(evaluation.verdict.results.len());
    for pv in evaluation.verdict.results.drain(..) {
        let mut reasons: Vec<Reason> = pv.verdict.reasons().to_vec();
        for module in &missing {
            reasons.push(Reason::UntrackedModuleMissing {
                module: module.clone(),
            });
        }
        let new_verdict = Verdict::from_reasons(reasons);
        updated_results.push(
            PinVerdict::new(pv.pin, new_verdict)
                .with_tracked_ref_details(pv.tracked_ref_details)
                .with_source_delta_details(pv.source_delta_details),
        );
    }
    evaluation.verdict = SeriesVerdict::from_results(updated_results);
    Ok(())
}

fn scan_erl_modules(repo_dir: &Path) -> CliResult<BTreeSet<String>> {
    const SKIP: &[&str] = &[
        ".git",
        "_build",
        "_rel",
        "logs",
        "node_modules",
        ".direnv",
        "target",
    ];
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<PathBuf> = vec![repo_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if SKIP.iter().any(|s| name_str == *s) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stem) = name_str.strip_suffix(".erl") {
                out.insert(stem.to_owned());
            }
        }
    }
    Ok(out)
}

fn resolve_rev_with_hint(g: &GitRepo, repo: &Path, rev: &str) -> CliResult<CommitSha> {
    g.resolve_rev(rev).map_err(|e| match e {
        GitError::CommitNotFound(_) => CliError::InvalidInput(format!(
            "commit {rev} not found in repository {}: did you forget to `git fetch`?",
            repo.display()
        )),
        other => CliError::Core(other.into()),
    })
}

fn load_source_files_at_parent(repo: &Path, commit: &str) -> CliResult<FileMap> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let to = resolve_rev_with_hint(&g, repo, commit)?;
    let parent = g.parent_commit(&to).map_err(|e| CliError::Core(e.into()))?;
    let Some(parent) = parent else {
        return Ok(FileMap::new());
    };
    load_files_at(&g, &parent)
}

fn range_source_files(repo: &Path, range: Option<&str>, merge: Option<&str>) -> CliResult<FileMap> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let from = match (range, merge) {
        (Some(r), None) => {
            let (a, _) = r
                .split_once("..")
                .ok_or_else(|| CliError::InvalidInput("--range expects BASE..HEAD".into()))?;
            resolve_rev_with_hint(&g, repo, a)?
        }
        (None, Some(merge_spec)) => {
            let merge = resolve_rev_with_hint(&g, repo, merge_spec)?;
            let parents = g.parents(&merge).map_err(|e| CliError::Core(e.into()))?;
            if parents.is_empty() {
                return Ok(FileMap::new());
            }
            parents[0].clone()
        }
        _ => return Ok(FileMap::new()),
    };
    load_files_at(&g, &from)
}

fn load_files_at(g: &GitRepo, commit: &CommitSha) -> CliResult<FileMap> {
    let blobs = g
        .read_paths_at_commit(commit, |p| p.ends_with(".erl") || p.ends_with(".hrl"))
        .map_err(|e| CliError::Core(e.into()))?;
    let mut map = FileMap::new();
    for blob in blobs {
        if let Ok(text) = str::from_utf8(&blob.bytes) {
            map.insert(blob.path, text.to_owned());
        }
    }
    Ok(map)
}

pub fn commit_patch_bytes(repo: &Path, commit: &str) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let to = resolve_rev_with_hint(&g, repo, commit)?;
    let parents = g.parents(&to).map_err(|e| CliError::Core(e.into()))?;
    // Don't silently first-parent a merge: hint at the explicit `check merge` verb.
    if parents.len() >= 2 {
        return Err(CliError::InvalidInput(format!(
            "{commit} is a merge commit ({} parents); use 'backhopper check merge {commit}' instead",
            parents.len()
        )));
    }
    let from = parents
        .into_iter()
        .next()
        .ok_or_else(|| CliError::InvalidInput(format!("commit {commit} has no parent")))?;
    let text = g
        .diff_commits_unified(&from, &to, |p| {
            p.ends_with(".erl") || p.ends_with(".hrl") || p.ends_with(".ex") || p.ends_with(".exs")
        })
        .map_err(|e| CliError::Core(e.into()))?;
    Ok(text.into_bytes())
}

fn range_patch_bytes(repo: &Path, range: Option<&str>, merge: Option<&str>) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let (from, to) = match (range, merge) {
        (Some(r), None) => {
            let (a, b) = r
                .split_once("..")
                .ok_or_else(|| CliError::InvalidInput(format!("invalid range {r:?}")))?;
            (
                resolve_rev_with_hint(&g, repo, a)?,
                resolve_rev_with_hint(&g, repo, b)?,
            )
        }
        (None, Some(merge_spec)) => {
            let merge = resolve_rev_with_hint(&g, repo, merge_spec)?;
            let parents = g.parents(&merge).map_err(|e| CliError::Core(e.into()))?;
            if parents.len() < 2 {
                return Err(CliError::InvalidInput(format!(
                    "{} is not a merge commit (parents: {})",
                    merge_spec,
                    parents.len()
                )));
            }
            (parents[0].clone(), merge)
        }
        _ => {
            return Err(CliError::InvalidInput(
                "specify either --range BASE..HEAD or --merge-commit SHA".into(),
            ));
        }
    };
    let text = g
        .diff_commits_unified(&from, &to, |p| {
            p.ends_with(".erl") || p.ends_with(".hrl") || p.ends_with(".ex") || p.ends_with(".exs")
        })
        .map_err(|e| CliError::Core(e.into()))?;
    Ok(text.into_bytes())
}

#[allow(clippy::too_many_arguments)]
fn run_check_patch(
    args: &GlobalArgs,
    cfg: &Config,
    bytes: &[u8],
    project: Option<ProjectName>,
    tag: Option<TagName>,
    series: Option<SeriesName>,
    repo_dir_path: Option<&Path>,
    source_files: &FileMap,
    source: &SourcePinArgs,
    target: &crate::cli::check::TargetRepoArgs,
    diagnostics: CheckFlags,
    merge_sha: Option<&str>,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let pin_specs: Vec<PinSpec> = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => vec![PinSpec::literal(p.clone(), t.clone())],
        (None, None, Some(s)) => {
            let s = cfg
                .series_by_name_with_coverage_check(s)
                .map_err(|e| CliError::Core(e.into()))?;
            s.pins.clone()
        }
        _ => {
            return Err(CliError::InvalidInput(
                "specify either --project + --tag, or --series".into(),
            ));
        }
    };
    let pins: Vec<Pin> = resolve_all_pin_specs(args, cfg, &store, &pin_specs, repo_dir_path)?;
    warn_on_stale_extractors(&coverage_report(cfg, &store, &pins));
    ensure_pin_snapshots_present(args, cfg, &store, &pins, diagnostics.auto_generate)?;
    let source_pins = resolve_source_pins(
        cfg,
        &store,
        &pins,
        project.as_ref(),
        tag.as_ref(),
        series.as_ref(),
        source,
    )?;
    let cache = SnapshotCache::new(&store);
    let mut evaluation = evaluate_one(
        cfg,
        &cache,
        bytes,
        &pins,
        &pin_specs,
        &source_pins,
        source_files,
        repo_dir_path,
    )?;
    if diagnostics.resolve_untracked_modules {
        let repo = repo_dir_path.ok_or_else(|| {
            CliError::InvalidInput("--resolve-untracked-modules requires --repo-dir-path".into())
        })?;
        resolve_untracked_modules_against_tree(&mut evaluation, repo)?;
    }
    promote_self_missing_to_prereq(
        &mut evaluation,
        cfg,
        &pin_specs,
        repo_dir_path,
        diagnostics.suggest_prereqs,
    );
    if let (Some(repo_path), Some(sha)) = (repo_dir_path, merge_sha) {
        let commit = CommitSha::new(sha.to_owned()).map_err(CoreError::from)?;
        let repo = GitRepo::open(repo_path).map_err(|e| CliError::Core(e.into()))?;
        evaluation.pr_commits =
            pr_commits_for(&repo, &commit).map_err(|e| CliError::Core(e.into()))?;
    }
    if let Some(target_ctx) =
        target_repo::build_context(target, &cfg.path_translations, repo_dir_path)?
    {
        let parsed = Patch::parse(bytes).map_err(|e| CliError::Core(e.into()))?;
        let touched: Vec<target_repo::TouchedPath<'_>> = parsed
            .files
            .iter()
            .filter_map(|f| {
                let path = f.new_path.as_deref().or(f.old_path.as_deref())?;
                let is_deletion =
                    f.new_path.is_none() || f.new_path.as_deref() == Some(Path::new("/dev/null"));
                Some(target_repo::TouchedPath { path, is_deletion })
            })
            .collect();
        let summary = target_repo::classify_touched_paths(&touched, &target_ctx);
        target_repo::merge_into_series_verdict(&summary, &mut evaluation.verdict);
    }
    let queried = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => QueriedAgainst::Pin {
            project: p.to_string(),
            tag: t.to_string(),
        },
        (None, None, Some(s)) => QueriedAgainst::Series {
            name: s.to_string(),
            pins: pins.iter().map(PinPayload::from).collect(),
        },
        _ => unreachable!("clap enforces either (--project + --tag) or --series"),
    };
    let known_projects: Vec<ProjectName> = cfg.projects.iter().map(|p| p.name.clone()).collect();
    let project_suggestions = if diagnostics.suggest_projects {
        build_suggestions(&evaluation.diagnostics, &known_projects)
    } else {
        Vec::new()
    };
    if diagnostics.write_suggestions && !project_suggestions.is_empty() {
        apply_suggestions_to_config(&cfg.config_path, &project_suggestions)?;
    }
    let payload = CompatPayload {
        queried_against: queried,
        results: evaluation.verdict.clone(),
        diagnostics: evaluation.diagnostics.clone(),
        project_suggestions: project_suggestions.clone(),
    };
    let ctx = OutputContext::new(args.formatter, "check patch");
    let exit = evaluation.worst_exit_code();
    if diagnostics.terse {
        return render_terse(&evaluation, exit);
    }
    if let Some(summary_fmt) = crate::commands::summary::SummaryFormatter::from_cli(args.formatter)
    {
        let sha = merge_sha
            .and_then(|s| CommitSha::new(s.to_owned()).ok())
            .unwrap_or_else(|| CommitSha::new("0".repeat(40)).unwrap());
        let subject = lookup_commit_subject(merge_sha, repo_dir_path).unwrap_or_default();
        let row = crate::commands::summary::to_summary_row(&evaluation, sha, subject);
        crate::commands::summary::emit_rows(summary_fmt, &[row])?;
        return Ok(exit);
    }
    let style = args.table_style;
    render_with_alts(
        &ctx,
        &payload,
        exit,
        |w| {
            render_text(w, &evaluation, &known_projects, diagnostics, style)?;
            render_suggestions_text(w, &project_suggestions, diagnostics)?;
            Ok(())
        },
        |w| {
            render_markdown_triage(w, &evaluation)?;
            Ok(())
        },
    )
}

pub fn render_markdown_triage(w: &mut dyn Write, evaluation: &SeriesEvaluation) -> CliResult<()> {
    writeln!(w, "| Pin | Verdict | Tracked refs | Notes |")?;
    writeln!(w, "| --- | --- | --- | --- |")?;
    for pv in &evaluation.verdict.results {
        let verdict_label = match &pv.verdict {
            Verdict::Compatible => "Compatible".to_owned(),
            Verdict::Inapplicable { reason } => {
                format!("Inapplicable ({})", reason.as_str())
            }
            Verdict::RequiresAdaptation { .. } => "RequiresAdaptation".to_owned(),
            Verdict::Incompatible { .. } => "Incompatible".to_owned(),
        };
        let notes = pv
            .verdict
            .reasons()
            .iter()
            .map(reason_md_label)
            .collect::<Vec<_>>()
            .join("; ");
        let notes = if notes.is_empty() {
            "—".to_owned()
        } else {
            notes
        };
        writeln!(
            w,
            "| {}@{} | {} | {} | {} |",
            pv.pin.project, pv.pin.tag, verdict_label, pv.tracked_refs, notes
        )?;
    }
    Ok(())
}

fn reason_md_label(r: &Reason) -> String {
    let sym = |k: &SymbolKind| -> String {
        match k {
            SymbolKind::Function { mfa } => mfa.to_string(),
            SymbolKind::Type {
                module,
                name,
                arity,
            } => format!("{module}:{name}/{arity}"),
            SymbolKind::Record { name } => format!("#{name}"),
            SymbolKind::Macro { name } => format!("?{name}"),
            SymbolKind::Behaviour { module } => format!("behaviour {module}"),
            SymbolKind::Callback {
                module,
                function,
                arity,
            } => {
                format!("callback {module}:{function}/{arity}")
            }
        }
    };
    match r {
        Reason::MissingSymbol { symbol, .. } => format!("MissingSymbol {}", sym(&symbol.kind)),
        Reason::ArityChanged {
            module, function, ..
        } => {
            format!("ArityChanged {module}:{function}")
        }
        Reason::SignatureChanged {
            module,
            function,
            arity,
            ..
        } => format!("SignatureChanged {module}:{function}/{arity}"),
        Reason::FileAbsent { path } => format!("FileAbsent {}", path.display()),
        Reason::ContextDrift { path, hunk_index } => {
            format!("ContextDrift {} hunk {}", path.display(), hunk_index)
        }
        Reason::DeprecatedUsage { symbol, .. } => format!("Deprecated {}", sym(&symbol.kind)),
        Reason::NowHidden { module } => format!("NowHidden {module}"),
        Reason::RecordFieldsChanged { record, .. } => format!("RecordFieldsChanged #{record}"),
        Reason::UnsupportedFileType { path } => format!("Unsupported {}", path.display()),
        Reason::UntrackedModuleMissing { module } => format!("UntrackedModuleMissing {module}"),
        Reason::ClauseMismatch {
            module,
            function,
            arity,
            ..
        } => format!("ClauseMismatch {module}:{function}/{arity}"),
        Reason::MissingPrereq { symbol, .. } => format!("MissingPrereq {}", sym(&symbol.kind)),
        Reason::SyntacticArtifact { path, line, .. } => {
            format!("SyntacticArtifact {}:{line}", path.display())
        }
        Reason::BehaviourCallbackSignatureChanged {
            behaviour,
            callback,
            arity,
            ..
        } => format!("CallbackSignatureChanged {behaviour}:{callback}/{arity}"),
        Reason::BehaviourCallbackRemoved {
            behaviour,
            callback,
            arity,
            ..
        } => format!("CallbackRemoved {behaviour}:{callback}/{arity}"),
        Reason::BehaviourCallbackAdded {
            behaviour,
            callback,
            arity,
            ..
        } => format!("CallbackAdded {behaviour}:{callback}/{arity}"),
        Reason::ModuleRelocated { module, .. } => format!("ModuleRelocated {module}"),
        Reason::WireConstantChanged {
            module, macro_name, ..
        } => format!("WireConstantChanged {module}.?{macro_name}"),
        Reason::HistoricalImplementationMissing {
            module,
            advertised_version_before,
            advertised_version_after,
            ..
        } => format!(
            "HistoricalImplementationMissing {module} {advertised_version_before}->{advertised_version_after}"
        ),
        Reason::WireContractBodyDrift {
            module,
            advertised_version,
            ..
        } => format!("WireContractBodyDrift {module} @ v{advertised_version}"),
        Reason::WireContractRegression {
            module,
            pin_version,
            patch_version,
        } => format!("WireContractRegression {module} {pin_version}->{patch_version}"),
        Reason::ReturnShapeMismatch {
            module,
            function,
            arity,
            ..
        } => format!("ReturnShapeMismatch {module}:{function}/{arity}"),
        Reason::MissingType {
            module,
            name,
            arity,
        } => format!("MissingType {module}:{name}/{arity}"),
        Reason::PreimageDrifted {
            path,
            hunk_index,
            line_delta,
        } => format!(
            "PreimageDrifted {} hunk #{hunk_index} Δ={line_delta:+}",
            path.display()
        ),
        Reason::PreimageMissing {
            path, hunk_index, ..
        } => format!("PreimageMissing {} hunk #{hunk_index}", path.display()),
        Reason::PathRename {
            source_path,
            target_path,
            translation,
        } => format!(
            "PathRename {} → {} (translation: {})",
            source_path.display(),
            target_path.display(),
            translation_name(translation),
        ),
        _ => format!("{r:?}"),
    }
}

fn translation_name(t: &TranslationSource) -> &str {
    match t {
        TranslationSource::ConfigStanza { name } | TranslationSource::ExternalFile { name, .. } => {
            name.as_str()
        }
    }
}

fn render_terse(evaluation: &SeriesEvaluation, exit: i32) -> CliResult<i32> {
    let summary_label = if evaluation.verdict.summary.incompatible > 0 {
        "incompatible"
    } else if evaluation.verdict.summary.requires_adaptation > 0 {
        "requires_adaptation"
    } else if evaluation.verdict.summary.compatible > 0 {
        "compatible"
    } else if evaluation.verdict.summary.inapplicable > 0 {
        "inapplicable"
    } else {
        "empty"
    };
    let pins = evaluation.verdict.results.len();
    let scope = dominant_scope(&evaluation.verdict.results);
    let line = serde_json::json!({
        "summary": summary_label,
        "pins": pins,
        "scope": scope,
        "exit": exit,
    });
    let mut out = io::stdout().lock();
    writeln!(out, "{line}").map_err(CliError::Io)?;
    Ok(exit)
}

fn dominant_scope(results: &[PinVerdict]) -> &'static str {
    // Every pin shares the same `TouchedKinds` (stamped from the patch once
    // in `evaluate_one`), so reading the first is enough.
    let Some(t) = results.first().map(|r| &r.touched) else {
        return "empty";
    };
    if t.erl > 0 || t.hrl > 0 {
        return "source";
    }
    let only_schema = t.schema > 0 && t.docs == 0 && t.tests == 0 && t.other == 0;
    if only_schema {
        return "schema_only";
    }
    let only_docs = t.docs > 0 && t.schema == 0 && t.tests == 0 && t.other == 0;
    if only_docs {
        return "docs_only";
    }
    let only_tests = t.tests > 0 && t.schema == 0 && t.docs == 0 && t.other == 0;
    if only_tests {
        return "tests_only";
    }
    if t.is_empty() {
        return "empty";
    }
    "mixed_non_source"
}

fn apply_suggestions_to_config(
    config_path: &Path,
    suggestions: &[ProjectSuggestion],
) -> CliResult<()> {
    let existing = fs::read_to_string(config_path).map_err(CliError::Io)?;
    let updated =
        append_suggestions_to_config(&existing, suggestions).map_err(CliError::InvalidInput)?;
    fs::write(config_path, updated).map_err(CliError::Io)?;
    // `tracing::info!` honors `--quiet` / `--verbose` levels via the global subscriber.
    tracing::info!(
        appended = suggestions.len(),
        config_path = %config_path.display(),
        "appended project stubs to config",
    );
    Ok(())
}

fn render_suggestions_text(
    w: &mut dyn Write,
    suggestions: &[ProjectSuggestion],
    flags: CheckFlags,
) -> CliResult<()> {
    if !flags.suggest_projects || suggestions.is_empty() || flags.summary_only {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(
        w,
        "Suggested [[project]] stubs (paste into backhopper.toml):"
    )?;
    for s in suggestions {
        writeln!(w)?;
        render_suggestion(w, s, 5).map_err(CliError::from)?;
    }
    if flags.write_suggestions {
        writeln!(w)?;
        writeln!(w, "(appended to config; set git_url and rerun)")?;
    }
    Ok(())
}

fn render_text(
    w: &mut dyn Write,
    evaluation: &SeriesEvaluation,
    known_projects: &[ProjectName],
    flags: CheckFlags,
    style: TableStyle,
) -> CliResult<()> {
    let v = &evaluation.verdict;
    if flags.summary_only {
        writeln!(
            w,
            "compatible={} requires_adaptation={} incompatible={} inapplicable={}",
            v.summary.compatible,
            v.summary.requires_adaptation,
            v.summary.incompatible,
            v.summary.inapplicable,
        )?;
        return Ok(());
    }
    writeln!(
        w,
        "compatible: {}, requires_adaptation: {}, incompatible: {}, inapplicable: {}",
        v.summary.compatible,
        v.summary.requires_adaptation,
        v.summary.incompatible,
        v.summary.inapplicable,
    )?;
    writeln!(w)?;
    writeln!(w, "{}", render_evaluation_table(evaluation, style))?;
    if flags.explain {
        render_explain_section(w, &evaluation.verdict.results)?;
    }
    let show_section = flags.show_untracked_calls || flags.show_otp_calls;
    if show_section && !evaluation.diagnostics.is_empty() {
        render_untracked_section(
            w,
            &evaluation.diagnostics,
            known_projects,
            flags.show_otp_calls,
        )?;
    }
    Ok(())
}

fn render_explain_section(w: &mut dyn Write, results: &[PinVerdict]) -> CliResult<()> {
    let any_refs = results.iter().any(|r| !r.tracked_ref_details.is_empty());
    let any_deltas = results.iter().any(|r| !r.source_delta_details.is_empty());
    if !any_refs && !any_deltas {
        return Ok(());
    }
    if any_refs {
        writeln!(w)?;
        writeln!(w, "tracked call sites per pin:")?;
        for r in results {
            if r.tracked_ref_details.is_empty() {
                continue;
            }
            writeln!(w, "  {} @ {}", r.pin.project, r.pin.tag)?;
            for sym in &r.tracked_ref_details {
                match &sym.kind {
                    SymbolKind::Function { mfa } => writeln!(w, "    {mfa}")?,
                    other => writeln!(w, "    {other:?}")?,
                }
            }
        }
    }
    if any_deltas {
        writeln!(w)?;
        writeln!(w, "source vs target spec drift per pin:")?;
        for r in results {
            if r.source_delta_details.is_empty() {
                continue;
            }
            writeln!(w, "  {} @ {}", r.pin.project, r.pin.tag)?;
            for d in &r.source_delta_details {
                writeln!(w, "    {}:{}/{}", d.module, d.function, d.arity)?;
                writeln!(w, "      source: {}", d.source_spec)?;
                writeln!(w, "      target: {}", d.target_spec)?;
            }
        }
    }
    Ok(())
}

fn render_untracked_section(
    w: &mut dyn Write,
    diagnostics: &Diagnostics,
    known_projects: &[ProjectName],
    show_otp: bool,
) -> CliResult<()> {
    let call_rows: Vec<(&ModuleName, usize, &'static str, String)> = diagnostics
        .untracked_calls
        .iter()
        .filter_map(|(module, count)| {
            let otp = is_otp_module(module);
            if otp && !show_otp {
                return None;
            }
            let (kind, hint) = annotate(module, otp, known_projects);
            Some((module, *count, kind, hint))
        })
        .collect();
    if !call_rows.is_empty() {
        writeln!(w)?;
        writeln!(
            w,
            "Untracked module calls (informational, not a verdict input):"
        )?;
        for (module, count, kind, hint) in call_rows {
            let plural = if count == 1 { "" } else { "s" };
            writeln!(
                w,
                "  {:<40} {} call{} {}",
                module,
                count,
                plural,
                format_annotation(kind, &hint),
            )?;
        }
    }
    if !diagnostics.untracked_records.is_empty() {
        writeln!(w)?;
        writeln!(w, "Untracked records (informational, not a verdict input):")?;
        for (record, count) in &diagnostics.untracked_records {
            let plural = if *count == 1 { "" } else { "s" };
            writeln!(w, "  #{record:<39} {count} reference{plural}")?;
        }
    }
    if !diagnostics.unanalyzed.is_empty() {
        writeln!(w)?;
        writeln!(
            w,
            "Unanalyzed dynamic calls (informational, not a verdict input):"
        )?;
        if diagnostics.unanalyzed.apply > 0 {
            writeln!(
                w,
                "  {:<40} {} (apply, spawn, spawn_link, spawn_monitor, spawn_opt, hibernate)",
                "apply-family BIFs", diagnostics.unanalyzed.apply,
            )?;
        }
        if diagnostics.unanalyzed.variable_dispatch > 0 {
            writeln!(
                w,
                "  {:<40} {} (Mod:fun(...), mod:F(...), Mod:F(...))",
                "variable-dispatch calls", diagnostics.unanalyzed.variable_dispatch,
            )?;
        }
    }
    Ok(())
}

fn annotate(
    module: &ModuleName,
    is_otp: bool,
    known_projects: &[ProjectName],
) -> (&'static str, String) {
    if is_otp {
        return ("OTP stdlib", String::new());
    }
    let name = module.as_str();
    let candidate = name
        .split_once('_')
        .map(|(prefix, _)| prefix)
        .unwrap_or(name);
    if known_projects.iter().any(|p| p.as_str() == candidate) {
        return ("tracked project, unscoped call", candidate.to_owned());
    }
    ("untracked project?", candidate.to_owned())
}

fn format_annotation(kind: &str, hint: &str) -> String {
    if hint.is_empty() {
        format!("({kind})")
    } else {
        format!("({kind}: {hint})")
    }
}

fn read_commits_file(path: &Path) -> CliResult<Vec<String>> {
    let raw = if path == Path::new("-") {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s).map_err(CliError::Io)?;
        s
    } else {
        fs::read_to_string(path).map_err(CliError::Io)?
    };
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn evaluate_one(
    cfg: &Config,
    cache: &SnapshotCache<'_>,
    bytes: &[u8],
    pins: &[Pin],
    pin_specs: &[PinSpec],
    source_pins: &[Option<Pin>],
    source_files: &FileMap,
    self_repo: Option<&Path>,
) -> CliResult<SeriesEvaluation> {
    let patch = Patch::parse(bytes).map_err(|e| CliError::Core(CoreError::Patch(e)))?;
    let touched_paths: Vec<PathBuf> = patch
        .files
        .iter()
        .filter_map(|f| f.new_path.clone().or_else(|| f.old_path.clone()))
        .collect();
    let mut macros_by_path: BTreeMap<PathBuf, MacroTable> = BTreeMap::new();
    for path in &touched_paths {
        if let Some(body) = source_files.get(path) {
            macros_by_path.insert(path.clone(), build_macro_table(body, path, source_files));
        }
    }
    let analyzed = patch.analyze_with_macros(&macros_by_path);
    let mut touched_kinds = TouchedKinds::from_paths(&touched_paths);
    touched_kinds.only_test_visibility = analyzed.is_only_test_visibility_change();
    let sibling_projects: Vec<&Project> = cfg.projects.iter().collect();
    let mut contexts = Vec::with_capacity(pins.len());
    let mut routings: Vec<PathRouting> = Vec::with_capacity(pins.len());
    for (idx, pin) in pins.iter().enumerate() {
        let project = cfg
            .project(&pin.project)
            .map_err(|e| CliError::Core(e.into()))?;
        routings.push(classify_paths_for_pin(
            &touched_paths,
            project,
            &sibling_projects,
        ));
        let snap_arc = cache
            .get(&pin.project, &pin.tag)
            .map_err(|e| CliError::Core(e.into()))?;
        let scope = build_pin_scope(project, &snap_arc);
        let pin_self_override = pin_specs.get(idx).and_then(PinSpec::self_repo_override);
        let files = build_pin_files(
            project,
            pin,
            &scope,
            &touched_paths,
            self_repo,
            pin_self_override,
        )?;
        let snap = (*snap_arc).clone();
        let mut ctx = EvaluationContext::new(pin.clone(), snap, scope).with_files(files);
        if let Some(Some(source_pin)) = source_pins.get(idx) {
            let source_snap_arc = cache
                .get(&source_pin.project, &source_pin.tag)
                .map_err(|e| CliError::Core(e.into()))?;
            ctx = ctx.with_source_snapshot((*source_snap_arc).clone());
        }
        contexts.push(ctx);
    }
    let mut eval = analyzed.evaluate_series(&contexts);
    if touched_kinds.erl > 0 || touched_kinds.hrl > 0 {
        touched_kinds.only_self_surface = is_self_only_evaluation(cfg, &eval.verdict.results);
    }
    let rewritten: Vec<PinVerdict> = eval
        .verdict
        .results
        .into_iter()
        .zip(routings)
        .map(|(pv, routing)| {
            let pv = pv.with_touched(touched_kinds);
            apply_path_routing(pv, &routing)
        })
        .collect();
    eval.verdict = SeriesVerdict::from_results(rewritten).promote_inapplicable();
    Ok(eval)
}

// Rewrites the verdict to Inapplicable when no touched path is in this pin's scope
pub fn apply_path_routing(pv: PinVerdict, routing: &PathRouting) -> PinVerdict {
    if routing.any_in_scope || !routing.has_any_attribution() {
        return pv;
    }
    let reason = pv.touched.inapplicable_reason().unwrap_or_else(|| {
        match routing.first_out_of_scope_owner() {
            Some(name) => InapplicableReason::OutOfScopeFor {
                project: name.clone(),
            },
            None => InapplicableReason::Untracked,
        }
    });
    PinVerdict {
        verdict: Verdict::Inapplicable { reason },
        ..pv
    }
}

/// True when there's at least one self-pin in the series and every
/// non-self pin saw zero in-scope references. The caller already
/// gated on "Erlang surface was touched" via `TouchedKinds::erl` or
/// `TouchedKinds::hrl`.
fn is_self_only_evaluation(cfg: &Config, results: &[PinVerdict]) -> bool {
    let mut saw_self = false;
    for pv in results {
        let Ok(project) = cfg.project(&pv.pin.project) else {
            return false;
        };
        if project.is_self() {
            saw_self = true;
        } else if pv.tracked_refs > 0 {
            return false;
        }
    }
    saw_self
}

fn resolve_pin_specs(specs: &[PinSpec], store: &SnapshotStore<ReadOnly>) -> CliResult<Vec<Pin>> {
    pin::resolve_all(specs, store).map_err(|e| CliError::Core(e.into()))
}

fn promote_self_missing_to_prereq(
    evaluation: &mut SeriesEvaluation,
    cfg: &Config,
    pin_specs: &[PinSpec],
    self_repo: Option<&Path>,
    suggest_prereqs: bool,
) {
    let self_project_name: Option<&ProjectName> = pin_specs
        .iter()
        .find_map(|s| if s.is_self() { Some(s.project()) } else { None });
    let Some(self_name) = self_project_name else {
        return;
    };
    let Some(self_branch_label) = pin_specs.iter().find_map(|s| match s {
        PinSpec::SelfRef { git_ref, .. } => Some(git_ref.clone()),
        _ => None,
    }) else {
        return;
    };
    let new_results: Vec<PinVerdict> = mem::take(&mut evaluation.verdict.results)
        .into_iter()
        .map(|pv| {
            if &pv.pin.project != self_name {
                return pv;
            }
            let has_missing_symbol = pv
                .verdict
                .reasons()
                .iter()
                .any(|r| matches!(r, Reason::MissingSymbol { .. }));
            if !has_missing_symbol {
                return pv;
            }
            let reasons: Vec<Reason> = pv
                .verdict
                .reasons()
                .iter()
                .map(|r| match r {
                    Reason::MissingSymbol { symbol, .. } => Reason::MissingPrereq {
                        symbol: symbol.clone(),
                        self_branch: self_branch_label.clone(),
                        suggested_source_for_prereq: if suggest_prereqs {
                            suggest_prereq_sha(symbol, cfg, self_repo)
                        } else {
                            None
                        },
                    },
                    other => other.clone(),
                })
                .collect();
            PinVerdict {
                verdict: Verdict::from_reasons(reasons),
                ..pv
            }
        })
        .collect();
    evaluation.verdict = SeriesVerdict::from_results(new_results);
}

// shells out to `git log -S`: `gix` lacks a pickaxe walker today
fn suggest_prereq_sha(
    symbol: &SymbolRef,
    _cfg: &Config,
    self_repo: Option<&Path>,
) -> Option<CommitSha> {
    let repo = self_repo?;
    let name = match &symbol.kind {
        SymbolKind::Function { mfa } => mfa.function.as_str().to_owned(),
        _ => return None,
    };
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "-S", &name, "--format=%H", "--max-count=1"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    if sha.is_empty() {
        return None;
    }
    CommitSha::new(sha).ok()
}

fn resolve_all_pin_specs(
    args: &GlobalArgs,
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    specs: &[PinSpec],
    self_repo: Option<&Path>,
) -> CliResult<Vec<Pin>> {
    let mut pins = Vec::with_capacity(specs.len());
    for spec in specs {
        if spec.is_self() {
            let pin = resolve_self_pin(self_repo, spec)?;
            let project = cfg
                .project(spec.project())
                .map_err(|e| CliError::Core(e.into()))?;
            ensure_self_snapshot_present(args, cfg, store, project, spec, self_repo, &pin)?;
            pins.push(pin);
        } else {
            pins.push(spec.resolve(store).map_err(|e| CliError::Core(e.into()))?);
        }
    }
    Ok(pins)
}

fn resolve_source_pins(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    target_pins: &[Pin],
    target_project: Option<&ProjectName>,
    target_tag: Option<&TagName>,
    target_series: Option<&SeriesName>,
    source: &SourcePinArgs,
) -> CliResult<Vec<Option<Pin>>> {
    if source.source_tag.is_none() && source.source_series.is_none() {
        return Ok(vec![None; target_pins.len()]);
    }
    if source.source_tag.is_some() && source.source_series.is_some() {
        return Err(CliError::InvalidInput(
            "specify either --source-tag or --source-series, not both".into(),
        ));
    }
    match (
        target_project,
        target_tag,
        target_series,
        &source.source_tag,
        &source.source_series,
    ) {
        (Some(p), Some(_), None, Some(src_tag), None) => {
            Ok(vec![Some(Pin::new(p.clone(), src_tag.clone()))])
        }
        (None, None, Some(_), None, Some(src_series_name)) => {
            let src_series = cfg
                .series_by_name(src_series_name)
                .map_err(|e| CliError::Core(e.into()))?;
            let src_pins = resolve_pin_specs(&src_series.pins, store)?;
            let mut queues: BTreeMap<ProjectName, VecDeque<Pin>> = BTreeMap::new();
            for sp in src_pins {
                queues.entry(sp.project.clone()).or_default().push_back(sp);
            }
            let mut out = Vec::with_capacity(target_pins.len());
            for target_pin in target_pins {
                let mapped = queues
                    .get_mut(&target_pin.project)
                    .and_then(|v| v.pop_front());
                out.push(mapped);
            }
            Ok(out)
        }
        _ => Err(CliError::InvalidInput(
            "--source-tag pairs with --project + --tag; --source-series pairs with --series".into(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_batch(
    args: &GlobalArgs,
    cfg: &Config,
    series_names: &[SeriesName],
    repo: &Path,
    commits_file_path: &Path,
    source: &SourcePinArgs,
    target: &crate::cli::check::TargetRepoArgs,
    diagnostics: CheckFlags,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let commits = read_commits_file(commits_file_path)?;
    let target_ctx = target_repo::build_context(target, &cfg.path_translations, Some(repo))?;
    if commits.is_empty() {
        return Err(CliError::InvalidInput(
            "no commits supplied (empty file or stdin)".into(),
        ));
    }
    let mut resolved_series: Vec<(SeriesName, Vec<Pin>, Vec<PinSpec>)> =
        Vec::with_capacity(series_names.len());
    for name in series_names {
        let s = cfg
            .series_by_name_with_coverage_check(name)
            .map_err(|e| CliError::Core(e.into()))?;
        let pins = resolve_all_pin_specs(args, cfg, &store, &s.pins, Some(repo))?;
        warn_on_stale_extractors(&coverage_report(cfg, &store, &pins));
        ensure_pin_snapshots_present(args, cfg, &store, &pins, diagnostics.auto_generate)?;
        resolved_series.push((name.clone(), pins, s.pins.clone()));
    }
    let queried: Vec<BatchQuery> = resolved_series
        .iter()
        .map(|(name, pins, _)| BatchQuery {
            series: name.clone(),
            pins: pins.iter().map(PinPayload::from).collect(),
        })
        .collect();
    let pair_count = commits.len() * resolved_series.len();
    let mut reporter = select_batch_reporter(args);
    reporter.start(pair_count, "evaluating commits");
    let mut results: Vec<BatchResult> = Vec::with_capacity(pair_count);
    let mut worst_exit: i32 = 0;
    let mut current: usize = 0;
    let cache = SnapshotCache::new(&store);
    for commit in &commits {
        let bytes = commit_patch_bytes(repo, commit)?;
        let source_files = load_source_files_at_parent(repo, commit)?;
        let commit_sha = CommitSha::new(commit.clone()).map_err(CoreError::from)?;
        for (name, pins, pin_specs) in &resolved_series {
            let item_label = format!("{commit} @ {name}");
            let source_pins =
                resolve_source_pins(cfg, &store, pins, None, None, Some(name), source)?;
            let mut evaluation = evaluate_one(
                cfg,
                &cache,
                &bytes,
                pins,
                pin_specs,
                &source_pins,
                &source_files,
                Some(repo),
            )?;
            if let Some(target_ctx) = &target_ctx {
                let parsed = Patch::parse(&bytes).map_err(|e| CliError::Core(e.into()))?;
                let touched: Vec<target_repo::TouchedPath<'_>> = parsed
                    .files
                    .iter()
                    .filter_map(|f| {
                        let path = f.new_path.as_deref().or(f.old_path.as_deref())?;
                        let is_deletion = f.new_path.is_none()
                            || f.new_path.as_deref() == Some(Path::new("/dev/null"));
                        Some(target_repo::TouchedPath { path, is_deletion })
                    })
                    .collect();
                let summary = target_repo::classify_touched_paths(&touched, target_ctx);
                target_repo::merge_into_series_verdict(&summary, &mut evaluation.verdict);
            }
            current += 1;
            reporter.progress(current, pair_count, &item_label);
            worst_exit = worst_exit.max(evaluation.worst_exit_code());
            results.push(BatchResult {
                commit: commit_sha.clone(),
                series: name.clone(),
                verdict: evaluation.verdict.clone(),
                diagnostics: evaluation.diagnostics.clone(),
                patch_facts: evaluation.patch_facts.clone(),
                touched_paths: evaluation.touched_paths.clone(),
                pr_commits: evaluation.pr_commits.clone(),
            });
        }
    }
    reporter.finish(pair_count);
    let payload = BatchPayload {
        queried_against: queried,
        results,
    };
    let ctx = OutputContext::new(args.formatter, "check batch");
    render_with_exit(&ctx, &payload, worst_exit, |w| {
        render_batch_text(w, &payload.results, diagnostics)
    })
}

#[derive(Debug, Serialize)]
struct MultiPayload {
    commit: String,
    queried_against: Vec<BatchQuery>,
    results: Vec<BatchResult>,
}

#[allow(clippy::too_many_arguments)]
fn run_multi(
    args: &GlobalArgs,
    cfg: &Config,
    series_names: &[SeriesName],
    repo: &Path,
    commit: &str,
    source: &SourcePinArgs,
    diagnostics: CheckFlags,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let bytes = commit_patch_bytes(repo, commit)?;
    let source_files = load_source_files_at_parent(repo, commit)?;
    let mut resolved_series: Vec<(SeriesName, Vec<Pin>, Vec<PinSpec>)> =
        Vec::with_capacity(series_names.len());
    for name in series_names {
        let s = cfg
            .series_by_name_with_coverage_check(name)
            .map_err(|e| CliError::Core(e.into()))?;
        let pins = resolve_all_pin_specs(args, cfg, &store, &s.pins, Some(repo))?;
        warn_on_stale_extractors(&coverage_report(cfg, &store, &pins));
        ensure_pin_snapshots_present(args, cfg, &store, &pins, diagnostics.auto_generate)?;
        resolved_series.push((name.clone(), pins, s.pins.clone()));
    }
    let queried: Vec<BatchQuery> = resolved_series
        .iter()
        .map(|(name, pins, _)| BatchQuery {
            series: name.clone(),
            pins: pins.iter().map(PinPayload::from).collect(),
        })
        .collect();
    let commit_sha = CommitSha::new(commit.to_owned()).map_err(CoreError::from)?;
    let mut results: Vec<BatchResult> = Vec::with_capacity(resolved_series.len());
    let mut worst_exit: i32 = 0;
    let cache = SnapshotCache::new(&store);
    for (name, pins, pin_specs) in &resolved_series {
        let source_pins = resolve_source_pins(cfg, &store, pins, None, None, Some(name), source)?;
        let evaluation = evaluate_one(
            cfg,
            &cache,
            &bytes,
            pins,
            pin_specs,
            &source_pins,
            &source_files,
            Some(repo),
        )?;
        worst_exit = worst_exit.max(evaluation.worst_exit_code());
        results.push(BatchResult {
            commit: commit_sha.clone(),
            series: name.clone(),
            verdict: evaluation.verdict.clone(),
            diagnostics: evaluation.diagnostics.clone(),
            patch_facts: evaluation.patch_facts.clone(),
            touched_paths: evaluation.touched_paths.clone(),
            pr_commits: evaluation.pr_commits.clone(),
        });
    }
    let payload = MultiPayload {
        commit: commit.to_owned(),
        queried_against: queried,
        results,
    };
    let ctx = OutputContext::new(args.formatter, "check multi");
    render_with_exit(&ctx, &payload, worst_exit, |w| {
        render_batch_text(w, &payload.results, diagnostics)
    })
}

fn select_batch_reporter(args: &GlobalArgs) -> Box<dyn ProgressReporter> {
    let stdout_is_tty = io::stdout().is_terminal();
    let stderr_is_tty = io::stderr().is_terminal();
    let any_tty = stdout_is_tty || stderr_is_tty;
    if matches!(args.formatter, Formatter::Json) || args.quiet || !any_tty {
        Box::new(QuietReporter::new())
    } else if args.non_interactive || !stderr_is_tty {
        Box::new(NonInteractiveReporter::new())
    } else {
        Box::new(InteractiveReporter::new())
    }
}

fn render_batch_text(
    w: &mut dyn Write,
    results: &[BatchResult],
    flags: CheckFlags,
) -> CliResult<()> {
    if flags.summary_only {
        for r in results {
            writeln!(
                w,
                "{} {} compatible={} requires_adaptation={} incompatible={} inapplicable={}",
                r.commit,
                r.series,
                r.verdict.summary.compatible,
                r.verdict.summary.requires_adaptation,
                r.verdict.summary.incompatible,
                r.verdict.summary.inapplicable,
            )?;
        }
        return Ok(());
    }
    let totals = results
        .iter()
        .fold((0u32, 0u32, 0u32, 0u32), |(c, ra, i, v), x| {
            (
                c + x.verdict.summary.compatible,
                ra + x.verdict.summary.requires_adaptation,
                i + x.verdict.summary.incompatible,
                v + x.verdict.summary.inapplicable,
            )
        });
    writeln!(
        w,
        "totals: compatible={} requires_adaptation={} incompatible={} inapplicable={}",
        totals.0, totals.1, totals.2, totals.3,
    )?;
    writeln!(w)?;
    for r in results {
        writeln!(
            w,
            "{}  {:<20} compatible={} requires_adaptation={} incompatible={} inapplicable={}",
            r.commit,
            r.series,
            r.verdict.summary.compatible,
            r.verdict.summary.requires_adaptation,
            r.verdict.summary.incompatible,
            r.verdict.summary.inapplicable,
        )?;
        if flags.explain {
            render_explain_section(w, &r.verdict.results)?;
        }
    }
    Ok(())
}
