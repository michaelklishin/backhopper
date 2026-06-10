// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::mem;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::str;

use bel7_cli::{
    InteractiveReporter, NonInteractiveReporter, ProgressReporter, QuietReporter, TableStyle,
};
use serde::Serialize;

use backhopper_core::Error as CoreError;
use backhopper_core::app_src::{AppSrcSpec, application_of_path, parse as app_src_parse};
use backhopper_core::compat::is_otp_module;
use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::routing::{PathRouting, classify_paths_for_pin};
use backhopper_core::compat::scope::{PinScope, parse_module_names};
use backhopper_core::compat::source_macros::{FileMap, build_macro_table};
use backhopper_core::config::{Config, Project};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::batch::{BatchPayload, BatchQuery, BatchResult, PinPayload};
use backhopper_core::model::names::{
    ApplicationName, CommitSha, CommitShaPrefix, DependencyName, ModuleName, ProjectName,
    SeriesName, TagName,
};
use backhopper_core::model::pin::{self, Pin, PinSpec};
use backhopper_core::model::pr_commit::PrCommit;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::summary::SummaryRow;
use backhopper_core::model::symbol::{SymbolKind, SymbolRef};
use backhopper_core::model::verdict::{
    AlreadyPresentSkipped, DepPinDivergence, Diagnostics, InapplicableReason, PinVerdict, Reason,
    SeriesEvaluation, SeriesVerdict, SnapshotSide, TargetMatch, TargetMatchKind, TouchedKinds,
    TranslationSource, Verdict,
};
use backhopper_core::store::{ReadOnly, SnapshotStore};

use backhopper_git::{
    CandidateIdentity, GitError, GitRepo, MergePolicy, PrCommitPolicy, ResolvedPatchInput,
    TargetWalkIndex, cherry_pick_trailers, load_files_at, normalized_patch_hash,
    trailer_origin_on_target,
};

use crate::cli::check::TargetRepoArgs;
use crate::cli::{CheckCmd, CheckFlags, Formatter, GlobalArgs, SourcePinArgs};
use crate::commands::auto_generate::{
    coverage_report, ensure_pin_snapshots_present, warn_on_stale_extractors,
};
use crate::commands::batch_plan::BatchPlan;
use crate::commands::context::{load_config, open_store_read};
use crate::commands::macro_env::macro_environment_hash;
use crate::commands::rabbitmq_components::{DepPin, parse_components_mk};
use crate::commands::self_snapshot::{ensure_self_snapshot_present, resolve_self_pin};
use crate::commands::sha_prefix::{enrich_with_repo_path, expand_prefix_with};
use crate::commands::snapshot_cache::SnapshotCache;
use crate::commands::suggest::{
    ProjectSuggestion, append_suggestions_to_config, build_suggestions, render_suggestion,
};
use crate::commands::summary::{
    SummaryFormatter, batch_result_to_summary_row, emit_rows, to_summary_row,
};
use crate::commands::target_repo;
use crate::commands::verdict_cache::{CacheLookupOutcome, CacheSession, MacroEnv, SessionLookup};
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
    /// Inner PR-branch commits for 2-parent merge inputs, `None` for
    /// everything else. The `None` vs `Some(vec![])` distinction is
    /// wire-load-bearing, so no `skip_serializing_if`.
    pr_commits: Option<Vec<PrCommit>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum QueriedAgainst {
    Pin { project: String, tag: String },
    Series { name: String, pins: Vec<PinPayload> },
}

/// Commit identity carried into `run_check_patch` for summary rows,
/// the `pr_commits` payload, and the verdict-cache key. Absent for
/// patch, range, and PR inputs, which have no single commit identity.
#[derive(Debug)]
struct PatchProvenance {
    sha: CommitSha,
    /// The commit the diff was computed against; the macro
    /// environment is read here on a cache miss.
    diff_base: CommitSha,
    parent_count: NonZeroU32,
    pr_commits: Option<Vec<PrCommit>>,
}

/// Patch bytes plus provenance for the per-commit check arms. The
/// macro file map is deliberately not here: it is loaded only when a
/// cache miss makes the evaluation pay for it.
#[derive(Debug)]
struct ResolvedForCheck {
    bytes: Vec<u8>,
    provenance: PatchProvenance,
}

fn resolve_commit_input(
    repo_dir_path: &Path,
    prefix: &CommitShaPrefix,
    merges: MergePolicy,
    pr: PrCommitPolicy,
) -> CliResult<ResolvedForCheck> {
    let repo = GitRepo::open(repo_dir_path.to_path_buf()).map_err(CliError::Git)?;
    let sha =
        expand_prefix_with(&repo, prefix).map_err(|e| enrich_with_repo_path(e, repo_dir_path))?;
    let input = ResolvedPatchInput::for_commit(&repo, &sha, merges, pr)?;
    let parent_count = input.source.parent_count();
    let ResolvedPatchInput {
        sha,
        diff_base,
        bytes,
        pr_commits,
        ..
    } = input;
    Ok(ResolvedForCheck {
        bytes,
        provenance: PatchProvenance {
            sha,
            diff_base,
            parent_count,
            pr_commits,
        },
    })
}

/// Where `run_check_patch` gets its macro file map: an eagerly built
/// map (patch, pr, and range inputs) or the diff base it can be read
/// from when an evaluation actually runs.
#[derive(Debug)]
enum SourceFilesInput<'a> {
    Eager(FileMap),
    AtDiffBase {
        repo_dir: &'a Path,
        diff_base: CommitSha,
    },
}

impl SourceFilesInput<'_> {
    /// Materialize the map. An empty patch touches nothing, so it
    /// skips the tree read, matching
    /// `ResolvedPatchInput::load_source_files`.
    fn load(self, bytes: &[u8]) -> CliResult<FileMap> {
        match self {
            Self::Eager(map) => Ok(map),
            Self::AtDiffBase { .. } if bytes.is_empty() => Ok(FileMap::new()),
            Self::AtDiffBase {
                repo_dir,
                diff_base,
            } => {
                let repo = GitRepo::open(repo_dir.to_path_buf()).map_err(CliError::Git)?;
                load_files_at(&repo, &diff_base).map_err(CliError::Git)
            }
        }
    }
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
                SourceFilesInput::Eager(FileMap::new()),
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
            let resolved = resolve_commit_input(
                &repo_dir_path,
                &commit,
                MergePolicy::Refuse,
                PrCommitPolicy::Skip,
            )?;
            run_check_patch(
                args,
                &cfg,
                &resolved.bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                SourceFilesInput::AtDiffBase {
                    repo_dir: &repo_dir_path,
                    diff_base: resolved.provenance.diff_base.clone(),
                },
                &source,
                &target,
                diagnostics,
                Some(resolved.provenance),
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
            if let Some(prefix) = merge_commit {
                let resolved = resolve_commit_input(
                    &repo_dir_path,
                    &prefix,
                    MergePolicy::RequireMerge,
                    PrCommitPolicy::Collect,
                )?;
                run_check_patch(
                    args,
                    &cfg,
                    &resolved.bytes,
                    project,
                    tag,
                    series,
                    Some(&repo_dir_path),
                    SourceFilesInput::AtDiffBase {
                        repo_dir: &repo_dir_path,
                        diff_base: resolved.provenance.diff_base.clone(),
                    },
                    &source,
                    &target,
                    diagnostics,
                    Some(resolved.provenance),
                )
            } else {
                let bytes = range_patch_bytes(&repo_dir_path, range.as_deref())?;
                let source_files = range_source_files(&repo_dir_path, range.as_deref())?;
                run_check_patch(
                    args,
                    &cfg,
                    &bytes,
                    project,
                    tag,
                    series,
                    Some(&repo_dir_path),
                    SourceFilesInput::Eager(source_files),
                    &source,
                    &target,
                    diagnostics,
                    None,
                )
            }
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
            let resolved = resolve_commit_input(
                &repo_dir_path,
                &merge_sha,
                MergePolicy::RequireMerge,
                PrCommitPolicy::Collect,
            )?;
            run_check_patch(
                args,
                &cfg,
                &resolved.bytes,
                project,
                tag,
                series,
                Some(&repo_dir_path),
                SourceFilesInput::AtDiffBase {
                    repo_dir: &repo_dir_path,
                    diff_base: resolved.provenance.diff_base.clone(),
                },
                &source,
                &target,
                diagnostics,
                Some(resolved.provenance),
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
                SourceFilesInput::Eager(FileMap::new()),
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

fn lookup_commit_subject(sha: &CommitSha, repo: Option<&Path>) -> Option<String> {
    let repo_path = repo?;
    let repo = GitRepo::open(repo_path).ok()?;
    repo.commit_subject(sha).ok()
}

#[allow(clippy::disallowed_methods)]
fn placeholder_zero_sha() -> CommitSha {
    CommitSha::new("0".repeat(40)).expect("40-char zero sha is valid")
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

#[allow(clippy::disallowed_methods)]
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
        let repo = GitRepo::open(repo_path).map_err(CliError::Git)?;
        (repo, commit)
    } else {
        let git_url = project
            .require_git_url()
            .map_err(|e| CliError::Core(e.into()))?
            .to_path_buf();
        let repo = GitRepo::open(git_url).map_err(CliError::Git)?;
        let commit = repo.resolve_tag(&pin.tag).map_err(CliError::Git)?;
        (repo, commit)
    };
    let needed: BTreeSet<String> = in_scope
        .iter()
        .map(|(_, project_path)| project_path.to_string_lossy().into_owned())
        .collect();
    let blobs = repo
        .read_paths_at_commit(&commit, |p| needed.contains(p))
        .map_err(CliError::Git)?;
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
        other => CliError::Git(other),
    })
}

fn range_source_files(repo: &Path, range: Option<&str>) -> CliResult<FileMap> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(CliError::Git)?;
    let Some(r) = range else {
        return Err(CliError::InvalidInput(
            "specify either --range BASE..HEAD or --merge-commit SHA".into(),
        ));
    };
    let (a, _) = r
        .split_once("..")
        .ok_or_else(|| CliError::InvalidInput("--range expects BASE..HEAD".into()))?;
    let from = resolve_rev_with_hint(&g, repo, a)?;
    let blobs = g
        .read_paths_at_commit(&from, |p| p.ends_with(".erl") || p.ends_with(".hrl"))
        .map_err(CliError::Git)?;
    let mut map = FileMap::new();
    for blob in blobs {
        if let Ok(text) = String::from_utf8(blob.bytes) {
            map.insert(blob.path, text);
        }
    }
    Ok(map)
}

fn range_patch_bytes(repo: &Path, range: Option<&str>) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(CliError::Git)?;
    let Some(r) = range else {
        return Err(CliError::InvalidInput(
            "specify either --range BASE..HEAD or --merge-commit SHA".into(),
        ));
    };
    let (a, b) = r
        .split_once("..")
        .ok_or_else(|| CliError::InvalidInput(format!("invalid range {r:?}")))?;
    let (from, to) = (
        resolve_rev_with_hint(&g, repo, a)?,
        resolve_rev_with_hint(&g, repo, b)?,
    );
    let text = g
        .diff_commits_unified(&from, &to, |p| {
            p.ends_with(".erl") || p.ends_with(".hrl") || p.ends_with(".ex") || p.ends_with(".exs")
        })
        .map_err(CliError::Git)?;
    Ok(text.into_bytes())
}

/// The content-key patch component. Empty diffs share one sentinel:
/// they are cached uniformly, keyed by everything else.
fn patch_hash_for_key(bytes: &[u8]) -> String {
    normalized_patch_hash(bytes).unwrap_or_else(|| "empty".to_owned())
}

/// The content-key macro component for a commit-shaped input: the
/// patch-reachable slice of the touched files' macro tables, rebuilt
/// at the diff base.
fn commit_macro_env(repo_dir: &Path, diff_base: &CommitSha, bytes: &[u8]) -> CliResult<MacroEnv> {
    if bytes.is_empty() {
        return Ok(MacroEnv::Resolved("no-touched-files".to_owned()));
    }
    let patch = Patch::parse(bytes).map_err(|e| CliError::Core(CoreError::Patch(e)))?;
    let repo = GitRepo::open(repo_dir.to_path_buf()).map_err(CliError::Git)?;
    Ok(
        match macro_environment_hash(&repo, diff_base, &patch.files) {
            Some(hash) => MacroEnv::Resolved(hash),
            None => MacroEnv::Unresolvable,
        },
    )
}

fn apply_target_context(
    cfg: &Config,
    target_ctx: &target_repo::TargetContext,
    bytes: &[u8],
    evaluation: &mut SeriesEvaluation,
) -> CliResult<()> {
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
    let summary = target_repo::classify_touched_paths(&touched, target_ctx);
    target_repo::merge_into_series_verdict(&summary, &mut evaluation.verdict);
    let search_globs = target_repo::collect_search_path_globs(cfg);
    let findings =
        target_repo::collect_added_file_findings(&parsed.files, &target_ctx.index, &search_globs);
    target_repo::merge_added_file_findings_into_evaluation(findings, evaluation);
    Ok(())
}

/// What tier-1 already-present detection concluded for one candidate.
enum AlreadyOnTarget {
    Match(TargetMatch),
    NoMatch,
    Skipped(AlreadyPresentSkipped),
}

/// Per-invocation already-present prober. Walk indexes are keyed by
/// merge base and shared across batch rows; candidates from one
/// source branch share a merge base in the common case.
struct AlreadyPresentProbe {
    target: GitRepo,
    tip: CommitSha,
    walk_limit: usize,
    source: GitRepo,
    indexes: RefCell<BTreeMap<CommitSha, Rc<TargetWalkIndex>>>,
}

impl AlreadyPresentProbe {
    fn maybe_open(
        target_ctx: Option<&target_repo::TargetContext>,
        source_repo: Option<&Path>,
    ) -> Option<Self> {
        let ctx = target_ctx?;
        if !ctx.already_present_enabled {
            return None;
        }
        let source = GitRepo::open(source_repo?.to_path_buf()).ok()?;
        let target = GitRepo::open(ctx.index.target_repo().to_path_buf()).ok()?;
        Some(Self {
            target,
            tip: ctx.index.resolved_commit().clone(),
            walk_limit: ctx.walk_limit,
            source,
            indexes: RefCell::new(BTreeMap::new()),
        })
    }

    /// Attach the tier-1 outcome to the evaluation's diagnostics.
    /// Detection failures degrade to a skipped-note, never an error:
    /// the advisory must not break the verdict pipeline.
    fn apply(&self, sha: &CommitSha, bytes: &[u8], evaluation: &mut SeriesEvaluation) {
        let candidate = self.candidate_identity(sha, bytes, evaluation);
        let outcome = self.detect(&candidate).unwrap_or_else(|e| {
            AlreadyOnTarget::Skipped(AlreadyPresentSkipped::TargetWalkFailed {
                detail: e.to_string(),
            })
        });
        match outcome {
            AlreadyOnTarget::Match(m) => {
                evaluation
                    .diagnostics
                    .already_present
                    .get_or_insert_default()
                    .identical = Some(m);
            }
            AlreadyOnTarget::NoMatch => {}
            AlreadyOnTarget::Skipped(s) => {
                evaluation.diagnostics.already_present_skipped = Some(s);
            }
        }
    }

    fn candidate_identity(
        &self,
        sha: &CommitSha,
        bytes: &[u8],
        evaluation: &SeriesEvaluation,
    ) -> CandidateIdentity {
        let trailer_origins = match self.source.commit_message(sha) {
            Ok(message) => cherry_pick_trailers(&message),
            Err(_) => Vec::new(),
        };
        let inner_pr_shas = evaluation
            .pr_commits
            .iter()
            .flatten()
            .map(|c| c.sha.clone())
            .collect();
        let touched_paths = evaluation
            .touched_paths
            .iter()
            .map(|p| PathBuf::from(p.as_str()))
            .collect();
        CandidateIdentity {
            sha: sha.clone(),
            trailer_origins,
            inner_pr_shas,
            patch_hash: normalized_patch_hash(bytes),
            touched_paths,
        }
    }

    fn detect(&self, candidate: &CandidateIdentity) -> Result<AlreadyOnTarget, GitError> {
        // Trailer-origin ancestry first: it needs no walk and works
        // even when the candidate object is absent from the target.
        if let Some(m) =
            trailer_origin_on_target(&self.target, &candidate.trailer_origins, &self.tip)?
        {
            return Ok(AlreadyOnTarget::Match(m));
        }
        if !self.target.has_commit(&candidate.sha)? {
            return Ok(AlreadyOnTarget::Skipped(
                AlreadyPresentSkipped::CandidateMissingFromTargetOdb {
                    sha: candidate.sha.clone(),
                },
            ));
        }
        let Some(since) = self.target.merge_base(&candidate.sha, &self.tip)? else {
            return Ok(AlreadyOnTarget::Skipped(AlreadyPresentSkipped::NoMergeBase));
        };
        let index = {
            let mut map = self.indexes.borrow_mut();
            match map.get(&since) {
                Some(index) => Rc::clone(index),
                None => {
                    let built = Rc::new(TargetWalkIndex::build(
                        &self.target,
                        &self.tip,
                        &since,
                        self.walk_limit,
                    )?);
                    map.insert(since, Rc::clone(&built));
                    built
                }
            }
        };
        Ok(match index.find_match(&self.target, candidate) {
            Some(m) => AlreadyOnTarget::Match(m),
            None => AlreadyOnTarget::NoMatch,
        })
    }
}

/// Per-invocation dep-pin divergence prober: target pins parsed
/// once, source pins and `.app.src` edges read per commit from git,
/// so every input the cached diagnostic depends on is keyed by the
/// commit or the target tip. The per-commit result is memoized: a
/// multi-series batch evaluates the same commit once per series.
struct DepPinProbe {
    source: GitRepo,
    target_pins: BTreeMap<String, DepPin>,
    by_commit: RefCell<BTreeMap<CommitSha, Vec<DepPinDivergence>>>,
}

impl DepPinProbe {
    fn maybe_open(
        target_ctx: Option<&target_repo::TargetContext>,
        source_repo: Option<&Path>,
    ) -> Option<Self> {
        let ctx = target_ctx?;
        let source_path = source_repo?;
        let target = GitRepo::open(ctx.index.target_repo().to_path_buf()).ok()?;
        let target_text = read_components_mk_blob(&target, ctx.index.resolved_commit())?;
        let target_pins = parse_components_mk(&target_text)
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();
        let source = GitRepo::open(source_path.to_path_buf()).ok()?;
        Some(Self {
            source,
            target_pins,
            by_commit: RefCell::new(BTreeMap::new()),
        })
    }

    /// Attach divergent pins reachable from the patch's touched apps.
    /// When no touched path resolves to an application the full
    /// divergent set is attached instead: coarse beats silent.
    fn apply(&self, sha: &CommitSha, evaluation: &mut SeriesEvaluation) {
        if let Some(memoized) = self.by_commit.borrow().get(sha) {
            evaluation
                .diagnostics
                .dep_pin_divergence
                .clone_from(memoized);
            return;
        }
        let divergence = self.divergence_for(sha, evaluation);
        evaluation
            .diagnostics
            .dep_pin_divergence
            .clone_from(&divergence);
        self.by_commit.borrow_mut().insert(sha.clone(), divergence);
    }

    fn divergence_for(
        &self,
        sha: &CommitSha,
        evaluation: &SeriesEvaluation,
    ) -> Vec<DepPinDivergence> {
        let Some(source_text) = read_components_mk_blob(&self.source, sha) else {
            return Vec::new();
        };
        let mut divergent: Vec<DepPinDivergence> = Vec::new();
        for pin in parse_components_mk(&source_text) {
            let Some(target_pin) = self.target_pins.get(&pin.name) else {
                continue;
            };
            if pin.version == target_pin.version && pin.source == target_pin.source {
                continue;
            }
            let Ok(dep) = DependencyName::new(pin.name.clone()) else {
                continue;
            };
            divergent.push(DepPinDivergence {
                dep,
                source: format!("{} {}", pin.source.label(), pin.version),
                target: format!("{} {}", target_pin.source.label(), target_pin.version),
            });
        }
        if divergent.is_empty() {
            return Vec::new();
        }
        let apps = self.apps_at_commit(sha);
        // Blob paths from git are repo-relative; an empty root keeps
        // them comparable with the equally relative touched paths.
        let repo_root = Path::new("");
        let touched_apps: BTreeSet<&ApplicationName> = evaluation
            .touched_paths
            .iter()
            .filter_map(|p| application_of_path(&apps, repo_root, Path::new(p.as_str())))
            .collect();
        if touched_apps.is_empty() {
            return divergent;
        }
        let reachable = reachable_app_names(&apps, &touched_apps);
        divergent
            .into_iter()
            .filter(|d| reachable.contains(d.dep.as_str()))
            .collect()
    }

    /// `.app.src` specs read from git at the commit: deterministic
    /// per key inputs, and works against bare clones.
    fn apps_at_commit(&self, sha: &CommitSha) -> Vec<AppSrcSpec> {
        let Ok(blobs) = self
            .source
            .read_paths_at_commit(sha, |p| p.ends_with(".app.src"))
        else {
            return Vec::new();
        };
        let mut specs = Vec::with_capacity(blobs.len());
        for blob in blobs {
            let Ok(text) = str::from_utf8(&blob.bytes) else {
                continue;
            };
            if let Ok(spec) = app_src_parse(&blob.path, text) {
                specs.push(spec);
            }
        }
        specs
    }
}

/// Transitive closure over `.app.src` `applications` edges, starting
/// from the touched apps. `rabbitmq_management` reaches `cowboy`
/// only through `rabbitmq_web_dispatch`.
fn reachable_app_names(
    apps: &[AppSrcSpec],
    start: &BTreeSet<&ApplicationName>,
) -> BTreeSet<String> {
    let by_name: BTreeMap<&str, &AppSrcSpec> = apps.iter().map(|a| (a.name.as_str(), a)).collect();
    let mut reachable: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<&str> = start.iter().map(|a| a.as_str()).collect();
    while let Some(name) = queue.pop_front() {
        if !reachable.insert(name.to_owned()) {
            continue;
        }
        if let Some(spec) = by_name.get(name) {
            for dep in &spec.applications {
                if !reachable.contains(dep.as_str()) {
                    queue.push_back(dep.as_str());
                }
            }
        }
    }
    reachable
}

fn read_components_mk_blob(repo: &GitRepo, commit: &CommitSha) -> Option<String> {
    let blobs = repo
        .read_paths_at_commit(commit, |p| p == "rabbitmq-components.mk")
        .ok()?;
    let blob = blobs.into_iter().next()?;
    String::from_utf8(blob.bytes).ok()
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
    source_files: SourceFilesInput<'_>,
    source: &SourcePinArgs,
    target: &TargetRepoArgs,
    diagnostics: CheckFlags,
    provenance: Option<PatchProvenance>,
) -> CliResult<i32> {
    debug_assert!(
        provenance.is_none() || repo_dir_path.is_some(),
        "commit-shaped inputs always carry a repo path"
    );
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
    let target_ctx = target_repo::build_context(target, &cfg.path_translations, repo_dir_path)?;
    // patch, pr, and range inputs have no stable content address; the
    // two flags rescan state that lives outside the cache key
    let cache_bypass = provenance.is_none()
        || diagnostics.resolve_untracked_modules
        || diagnostics.suggest_prereqs;
    let session = CacheSession::open(args, cfg, diagnostics.no_cache, cache_bypass);
    let key = provenance.as_ref().and_then(|p| {
        session.check_key(
            cfg,
            &store,
            &p.sha,
            series.as_ref(),
            &pins,
            &pin_specs,
            &source_pins,
            target_ctx.as_ref(),
        )
    });
    let cache = SnapshotCache::new(&store);
    let evaluate_and_finish = |files: FileMap| -> CliResult<SeriesEvaluation> {
        let mut evaluation = evaluate_one(
            cfg,
            &cache,
            bytes,
            &pins,
            &pin_specs,
            &source_pins,
            &files,
            repo_dir_path,
        )?;
        if diagnostics.resolve_untracked_modules {
            let repo = repo_dir_path.ok_or_else(|| {
                CliError::InvalidInput(
                    "--resolve-untracked-modules requires --repo-dir-path".into(),
                )
            })?;
            resolve_untracked_modules_against_tree(&mut evaluation, repo)?;
        }
        if let Some(p) = &provenance {
            evaluation.pr_commits.clone_from(&p.pr_commits);
        }
        if let Some(target_ctx) = &target_ctx {
            apply_target_context(cfg, target_ctx, bytes, &mut evaluation)?;
            // Patch-shaped inputs have no commit identity to probe.
            if let Some(p) = &provenance {
                if let Some(probe) =
                    AlreadyPresentProbe::maybe_open(Some(target_ctx), repo_dir_path)
                {
                    probe.apply(&p.sha, bytes, &mut evaluation);
                }
                if let Some(probe) = DepPinProbe::maybe_open(Some(target_ctx), repo_dir_path) {
                    probe.apply(&p.sha, &mut evaluation);
                }
            }
        }
        Ok(evaluation)
    };
    let mut evaluation = match session.lookup(key) {
        SessionLookup::Hit(evaluation) => *evaluation,
        SessionLookup::Bypassed => evaluate_and_finish(source_files.load(bytes)?)?,
        SessionLookup::Miss(miss) => {
            // a key exists, so provenance and a repo path do too
            let p = provenance
                .as_ref()
                .expect("cache keys are derived from provenance");
            let repo_dir = repo_dir_path.expect("commit-shaped inputs always carry a repo path");
            let macro_env = commit_macro_env(repo_dir, &p.diff_base, bytes)?;
            match session.consult_content(*miss, patch_hash_for_key(bytes), &macro_env) {
                CacheLookupOutcome::Hit(evaluation) => *evaluation,
                CacheLookupOutcome::Miss(slot) => {
                    let evaluation = evaluate_and_finish(source_files.load(bytes)?)?;
                    slot.store(&evaluation);
                    evaluation
                }
            }
        }
    };
    session.report();
    // prereq promotion stays out of the cached value so batch rows
    // and check rows share entries; it commutes with the target merge
    // (it only rewrites MissingSymbol reasons)
    promote_self_missing_to_prereq(
        &mut evaluation,
        cfg,
        &pin_specs,
        repo_dir_path,
        diagnostics.suggest_prereqs,
    );
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
        pr_commits: evaluation.pr_commits.clone(),
        project_suggestions: project_suggestions.clone(),
    };
    let ctx = OutputContext::new(args.formatter, "check patch");
    let exit = evaluation.worst_exit_code();
    if diagnostics.terse {
        return render_terse(&evaluation, exit);
    }
    if let Some(summary_fmt) = SummaryFormatter::from_cli(args.formatter) {
        let (sha, subject) = match &provenance {
            Some(p) => (
                p.sha.clone(),
                lookup_commit_subject(&p.sha, repo_dir_path).unwrap_or_default(),
            ),
            None => (placeholder_zero_sha(), String::new()),
        };
        let row = to_summary_row(
            &evaluation,
            sha,
            subject,
            series.clone(),
            provenance.as_ref().map(|p| p.parent_count),
        );
        emit_rows(summary_fmt, &[row])?;
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
        Reason::VersionedMachineSnapshotMissing { module, side } => {
            format!(
                "VersionedMachineSnapshotMissing {module} ({})",
                side_label(*side)
            )
        }
        Reason::WireConstantBindingsMissing {
            module,
            macros,
            side,
        } => {
            let names: Vec<String> = macros.iter().map(|m| format!("?{m}")).collect();
            format!(
                "WireConstantBindingsMissing {module} [{}] ({})",
                names.join(", "),
                side_label(*side),
            )
        }
        _ => format!("{r:?}"),
    }
}

fn side_label(s: SnapshotSide) -> &'static str {
    match s {
        SnapshotSide::Source => "source",
        SnapshotSide::Target => "target",
        SnapshotSide::Both => "both",
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
    render_already_present(w, &evaluation.diagnostics)?;
    for d in &evaluation.diagnostics.dep_pin_divergence {
        writeln!(
            w,
            "dep pin divergence: {} {} (source) vs {} (target)",
            d.dep, d.source, d.target
        )?;
    }
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

/// Always-on advisory lines: an already-present patch changes what
/// the operator does next, so it never hides behind a flag.
fn render_already_present(w: &mut dyn Write, diagnostics: &Diagnostics) -> CliResult<()> {
    if let Some(ap) = &diagnostics.already_present {
        if let Some(m) = &ap.identical {
            let via = match m.via {
                TargetMatchKind::TrailerIntersection => "trailer intersection",
                TargetMatchKind::TrailerAncestry => "trailer ancestry",
                TargetMatchKind::PatchId => "patch id",
            };
            writeln!(
                w,
                "already present on target: identical patch {} via {via} (\"{}\")",
                m.commit, m.subject
            )?;
        }
        if let Some(c) = &ap.content {
            let strength = if c.fully_present() {
                "all hunks already present"
            } else {
                "content partially present"
            };
            writeln!(
                w,
                "{strength} on {}: {}/{} hunks applied ({} ambiguous, {} low-confidence)",
                c.pin,
                c.hunks_already_applied,
                c.hunks_considered,
                c.hunks_ambiguous,
                c.hunks_low_confidence,
            )?;
        }
    }
    if let Some(skipped) = &diagnostics.already_present_skipped {
        match skipped {
            AlreadyPresentSkipped::CandidateMissingFromTargetOdb { sha } => writeln!(
                w,
                "already-present detection skipped: candidate {sha} missing from target object store"
            )?,
            AlreadyPresentSkipped::NoMergeBase => writeln!(
                w,
                "already-present detection skipped: no merge base with the target tip"
            )?,
            AlreadyPresentSkipped::TargetWalkFailed { detail } => writeln!(
                w,
                "already-present detection skipped: target walk failed ({detail})"
            )?,
            _ => writeln!(w, "already-present detection skipped")?,
        }
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

fn read_commits_file(path: &Path) -> CliResult<Vec<(usize, CommitShaPrefix)>> {
    let raw = if path == Path::new("-") {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s).map_err(CliError::Io)?;
        s
    } else {
        fs::read_to_string(path).map_err(CliError::Io)?
    };
    let raw = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
    let mut out = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (idx, raw_line) in raw.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let token = match trimmed.split_once('#') {
            Some((before, _)) => before.trim(),
            None => trimmed,
        };
        if token.is_empty() {
            continue;
        }
        match token.parse::<CommitShaPrefix>() {
            Ok(prefix) => out.push((line_no, prefix)),
            Err(e) => errors.push(format!("line {line_no}: {e}")),
        }
    }
    if !errors.is_empty() {
        let detail = errors.join("\n  ");
        return Err(CliError::InvalidInput(format!(
            "invalid commits file {}:\n  {detail}",
            path.display()
        )));
    }
    Ok(out)
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
        let mut ctx = EvaluationContext::new(pin.clone(), snap, scope)
            .with_files(files)
            .with_family_defaults(project.family.defaults());
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
#[allow(clippy::disallowed_methods)]
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
    target: &TargetRepoArgs,
    diagnostics: CheckFlags,
) -> CliResult<i32> {
    reject_terse(diagnostics, "check batch")?;
    let store = open_store_read(args, cfg)?;
    let entries = read_commits_file(commits_file_path)?;
    let target_ctx = target_repo::build_context(target, &cfg.path_translations, Some(repo))?;
    if entries.is_empty() {
        return Err(CliError::InvalidInput(
            "no commits supplied (empty file or stdin)".into(),
        ));
    }
    let git_repo = GitRepo::open(repo.to_path_buf()).map_err(CliError::Git)?;
    let probe = AlreadyPresentProbe::maybe_open(target_ctx.as_ref(), Some(repo));
    let dep_pin_probe = DepPinProbe::maybe_open(target_ctx.as_ref(), Some(repo));
    let plan = BatchPlan::resolve(&git_repo, repo, &entries)?;
    let resolved_series = resolve_series_set(args, cfg, &store, series_names, repo, diagnostics)?;
    let queried = batch_queries(&resolved_series);
    let pair_count = plan.len() * resolved_series.len();
    let mut reporter = select_batch_reporter(args);
    reporter.start(pair_count, "evaluating commits");
    let mut results: Vec<BatchResult> = Vec::with_capacity(pair_count);
    let mut worst_exit: i32 = 0;
    let mut current: usize = 0;
    let cache = SnapshotCache::new(&store);
    let session = CacheSession::open(args, cfg, diagnostics.no_cache, false);
    // Source pins depend only on the series, never on the commit.
    let mut source_pins_by_series = Vec::with_capacity(resolved_series.len());
    for series in &resolved_series {
        source_pins_by_series.push(resolve_source_pins(
            cfg,
            &store,
            &series.pins,
            None,
            None,
            Some(&series.name),
            source,
        )?);
    }
    for planned in plan.commits() {
        // resolved lazily: only a miss pays for the diff, the macro
        // environment, or the file map
        let mut input: Option<ResolvedPatchInput> = None;
        let mut source_files: Option<FileMap> = None;
        let mut macro_env: Option<MacroEnv> = None;
        let merge_marker = if planned.parents.len() > 1 {
            " (merge)"
        } else {
            ""
        };
        for (series, source_pins) in resolved_series.iter().zip(&source_pins_by_series) {
            let item_label = format!("{} @ {}{merge_marker}", planned.sha, series.name);
            let key = session.check_key(
                cfg,
                &store,
                &planned.sha,
                Some(&series.name),
                &series.pins,
                &series.pin_specs,
                source_pins,
                target_ctx.as_ref(),
            );
            let lookup = session.lookup(key);
            if !matches!(lookup, SessionLookup::Hit(_)) && input.is_none() {
                input = Some(ResolvedPatchInput::from_parents(
                    &git_repo,
                    &planned.sha,
                    &planned.parents,
                    MergePolicy::FirstParentDiff,
                    PrCommitPolicy::Collect,
                )?);
            }
            let evaluate = |source_files: &mut Option<FileMap>| -> CliResult<SeriesEvaluation> {
                let input = input.as_ref().expect("resolved before any evaluation");
                if source_files.is_none() {
                    *source_files =
                        Some(input.load_source_files(&git_repo).map_err(CliError::Git)?);
                }
                let files = source_files.as_ref().expect("loaded above");
                let mut evaluation = evaluate_one(
                    cfg,
                    &cache,
                    &input.bytes,
                    &series.pins,
                    &series.pin_specs,
                    source_pins,
                    files,
                    Some(repo),
                )?;
                evaluation.pr_commits.clone_from(&input.pr_commits);
                if let Some(target_ctx) = &target_ctx {
                    apply_target_context(cfg, target_ctx, &input.bytes, &mut evaluation)?;
                    if let Some(probe) = &probe {
                        probe.apply(&planned.sha, &input.bytes, &mut evaluation);
                    }
                    if let Some(probe) = &dep_pin_probe {
                        probe.apply(&planned.sha, &mut evaluation);
                    }
                }
                Ok(evaluation)
            };
            let evaluation = match lookup {
                SessionLookup::Hit(evaluation) => *evaluation,
                SessionLookup::Bypassed => evaluate(&mut source_files)?,
                SessionLookup::Miss(miss) => {
                    let resolved = input.as_ref().expect("resolved on the miss path");
                    if macro_env.is_none() {
                        macro_env = Some(commit_macro_env(
                            repo,
                            &resolved.diff_base,
                            &resolved.bytes,
                        )?);
                    }
                    let env = macro_env.as_ref().expect("computed above");
                    match session.consult_content(*miss, patch_hash_for_key(&resolved.bytes), env) {
                        CacheLookupOutcome::Hit(evaluation) => *evaluation,
                        CacheLookupOutcome::Miss(slot) => {
                            let evaluation = evaluate(&mut source_files)?;
                            slot.store(&evaluation);
                            evaluation
                        }
                    }
                }
            };
            current += 1;
            reporter.progress(current, pair_count, &item_label);
            worst_exit = worst_exit.max(evaluation.worst_exit_code());
            results.push(BatchResult {
                commit: planned.sha.clone(),
                series: series.name.clone(),
                pr_commits: evaluation.pr_commits.clone(),
                verdict: evaluation.verdict,
                diagnostics: evaluation.diagnostics,
                patch_facts: evaluation.patch_facts,
                touched_paths: evaluation.touched_paths,
                parent_count: NonZeroU32::new(planned.parents.len() as u32),
            });
        }
    }
    reporter.finish(pair_count);
    session.report();
    let payload = BatchPayload {
        queried_against: queried,
        results,
    };
    if let Some(summary_fmt) = SummaryFormatter::from_cli(args.formatter) {
        let rows = batch_summary_rows(&git_repo, &payload.results);
        emit_rows(summary_fmt, &rows)?;
        return Ok(worst_exit);
    }
    let ctx = OutputContext::new(args.formatter, "check batch");
    render_with_exit(&ctx, &payload, worst_exit, |w| {
        render_batch_text(w, &payload.results, diagnostics)
    })
}

/// One requested series with its resolved pins and original specs.
struct ResolvedSeries {
    name: SeriesName,
    pins: Vec<Pin>,
    pin_specs: Vec<PinSpec>,
}

/// Resolve and snapshot-check every requested series once, up front.
fn resolve_series_set(
    args: &GlobalArgs,
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    series_names: &[SeriesName],
    repo: &Path,
    diagnostics: CheckFlags,
) -> CliResult<Vec<ResolvedSeries>> {
    let mut resolved = Vec::with_capacity(series_names.len());
    for name in series_names {
        let s = cfg
            .series_by_name_with_coverage_check(name)
            .map_err(|e| CliError::Core(e.into()))?;
        let pins = resolve_all_pin_specs(args, cfg, store, &s.pins, Some(repo))?;
        warn_on_stale_extractors(&coverage_report(cfg, store, &pins));
        ensure_pin_snapshots_present(args, cfg, store, &pins, diagnostics.auto_generate)?;
        resolved.push(ResolvedSeries {
            name: name.clone(),
            pins,
            pin_specs: s.pins.clone(),
        });
    }
    Ok(resolved)
}

fn batch_queries(resolved_series: &[ResolvedSeries]) -> Vec<BatchQuery> {
    resolved_series
        .iter()
        .map(|s| BatchQuery {
            series: s.name.clone(),
            pins: s.pins.iter().map(PinPayload::from).collect(),
        })
        .collect()
}

/// One summary row per `(commit, series)` pair, with the commit
/// subject looked up once per distinct commit.
fn batch_summary_rows(repo: &GitRepo, results: &[BatchResult]) -> Vec<SummaryRow> {
    let mut subjects: BTreeMap<&CommitSha, String> = BTreeMap::new();
    results
        .iter()
        .map(|r| {
            let subject = subjects
                .entry(&r.commit)
                .or_insert_with(|| repo.commit_subject(&r.commit).unwrap_or_default())
                .clone();
            batch_result_to_summary_row(r, subject)
        })
        .collect()
}

fn reject_terse(diagnostics: CheckFlags, verb: &str) -> CliResult<()> {
    if diagnostics.terse {
        return Err(CliError::InvalidInput(format!(
            "--terse is not supported on `{verb}`; use `--formatter summary` for one JSON row per (commit, series) pair"
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct MultiPayload {
    commit: CommitSha,
    queried_against: Vec<BatchQuery>,
    results: Vec<BatchResult>,
}

#[allow(clippy::too_many_arguments)]
fn run_multi(
    args: &GlobalArgs,
    cfg: &Config,
    series_names: &[SeriesName],
    repo: &Path,
    commit: &CommitShaPrefix,
    source: &SourcePinArgs,
    diagnostics: CheckFlags,
) -> CliResult<i32> {
    reject_terse(diagnostics, "check multi")?;
    let store = open_store_read(args, cfg)?;
    let git_repo = GitRepo::open(repo.to_path_buf()).map_err(CliError::Git)?;
    let commit =
        &expand_prefix_with(&git_repo, commit).map_err(|e| enrich_with_repo_path(e, repo))?;
    let parents = git_repo.parents(commit).map_err(CliError::Git)?;
    let resolved_series = resolve_series_set(args, cfg, &store, series_names, repo, diagnostics)?;
    let queried = batch_queries(&resolved_series);
    let mut results: Vec<BatchResult> = Vec::with_capacity(resolved_series.len());
    let mut worst_exit: i32 = 0;
    let cache = SnapshotCache::new(&store);
    let session = CacheSession::open(args, cfg, diagnostics.no_cache, false);
    // resolved lazily: only a miss pays for the diff, the macro
    // environment, or the file map
    let mut input: Option<ResolvedPatchInput> = None;
    let mut source_files: Option<FileMap> = None;
    let mut macro_env: Option<MacroEnv> = None;
    for series in &resolved_series {
        let source_pins = resolve_source_pins(
            cfg,
            &store,
            &series.pins,
            None,
            None,
            Some(&series.name),
            source,
        )?;
        let key = session.check_key(
            cfg,
            &store,
            commit,
            Some(&series.name),
            &series.pins,
            &series.pin_specs,
            &source_pins,
            None,
        );
        let lookup = session.lookup(key);
        if !matches!(lookup, SessionLookup::Hit(_)) && input.is_none() {
            input = Some(ResolvedPatchInput::from_parents(
                &git_repo,
                commit,
                &parents,
                MergePolicy::FirstParentDiff,
                PrCommitPolicy::Collect,
            )?);
        }
        let evaluate = |source_files: &mut Option<FileMap>| -> CliResult<SeriesEvaluation> {
            let input = input.as_ref().expect("resolved before any evaluation");
            if source_files.is_none() {
                *source_files = Some(input.load_source_files(&git_repo).map_err(CliError::Git)?);
            }
            let files = source_files.as_ref().expect("loaded above");
            let mut evaluation = evaluate_one(
                cfg,
                &cache,
                &input.bytes,
                &series.pins,
                &series.pin_specs,
                &source_pins,
                files,
                Some(repo),
            )?;
            evaluation.pr_commits.clone_from(&input.pr_commits);
            Ok(evaluation)
        };
        let evaluation = match lookup {
            SessionLookup::Hit(evaluation) => *evaluation,
            SessionLookup::Bypassed => evaluate(&mut source_files)?,
            SessionLookup::Miss(miss) => {
                let resolved = input.as_ref().expect("resolved on the miss path");
                if macro_env.is_none() {
                    macro_env = Some(commit_macro_env(
                        repo,
                        &resolved.diff_base,
                        &resolved.bytes,
                    )?);
                }
                let env = macro_env.as_ref().expect("computed above");
                match session.consult_content(*miss, patch_hash_for_key(&resolved.bytes), env) {
                    CacheLookupOutcome::Hit(evaluation) => *evaluation,
                    CacheLookupOutcome::Miss(slot) => {
                        let evaluation = evaluate(&mut source_files)?;
                        slot.store(&evaluation);
                        evaluation
                    }
                }
            }
        };
        worst_exit = worst_exit.max(evaluation.worst_exit_code());
        results.push(BatchResult {
            commit: commit.clone(),
            series: series.name.clone(),
            pr_commits: evaluation.pr_commits.clone(),
            verdict: evaluation.verdict,
            diagnostics: evaluation.diagnostics,
            patch_facts: evaluation.patch_facts,
            touched_paths: evaluation.touched_paths,
            parent_count: NonZeroU32::new(parents.len() as u32),
        });
    }
    session.report();
    let payload = MultiPayload {
        commit: commit.clone(),
        queried_against: queried,
        results,
    };
    if let Some(summary_fmt) = SummaryFormatter::from_cli(args.formatter) {
        let rows = batch_summary_rows(&git_repo, &payload.results);
        emit_rows(summary_fmt, &rows)?;
        return Ok(worst_exit);
    }
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
