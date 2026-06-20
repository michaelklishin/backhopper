// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! CLI-side helpers for the `--target-repo-dir-path` cross-branch
//! analyser. Builds the `TargetTreeIndex` and merged translations
//! once per invocation, then applies the per-touched-path classifier
//! to every pin's verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use backhopper_core::compat::added_file::{AddedFileFindings, analyse_added_files};
use backhopper_core::compat::classify_hunks_against_target;
use backhopper_core::compat::define_resolve::{DefineSubject, analyse_define_symbols};
use backhopper_core::compat::local_call_resolve::{LocalCallSubject, analyse_local_calls};
use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::compat::{
    TargetPathClassification, TouchedPathQuery, classify_path, normalise,
};
use backhopper_core::config::{Config, PathTranslations};
use backhopper_core::model::names::{CommitSha, GitRef, ModuleName, RelativePath};
use backhopper_core::model::verdict::{
    Diagnostics, InapplicableReason, PinVerdict, Reason, SeriesEvaluation, SeriesSummary,
    SeriesVerdict, TranslationSource, Verdict,
};
use backhopper_git::{GitRepo, build_target_tree_index};

use crate::cli::check::TargetRepoArgs;
use crate::errors::{CliError, CliResult};

#[derive(Debug)]
pub struct TargetContext {
    pub index: TargetTreeIndex,
    pub translations: PathTranslations,
    /// Already-present walk cap; joins the verdict-cache key because
    /// it changes the diagnostic output.
    pub walk_limit: usize,
    /// Already-present detection toggle; joins the key for the same
    /// reason.
    pub already_present_enabled: bool,
}

pub fn build_context(
    args: &TargetRepoArgs,
    base_translations: &PathTranslations,
    source_repo: Option<&Path>,
) -> CliResult<Option<TargetContext>> {
    let Some(target_repo) = &args.target_repo_dir_path else {
        return Ok(None);
    };
    let mut translations = base_translations.clone();
    if let Some(file) = &args.path_translations_file_path {
        let external =
            PathTranslations::load_external(file).map_err(|e| CliError::Core(e.into()))?;
        translations
            .merge_external(external)
            .map_err(|e| CliError::Core(e.into()))?;
    }
    let repo = GitRepo::open(target_repo.clone()).map_err(CliError::Git)?;
    let target_ref = GitRef::new(args.target_ref.clone()).map_err(|e| CliError::Core(e.into()))?;
    let index = build_target_tree_index(&repo, &target_ref).map_err(CliError::Git)?;

    if let Some(src) = source_repo {
        if same_repo(src, target_repo) {
            if let Ok(src_repo) = GitRepo::open(src.to_path_buf()) {
                if let Ok(src_commit) = src_repo.resolve_rev(args.target_ref.as_str()) {
                    if &src_commit == index.resolved_commit() {
                        tracing::warn!(
                            "target-repo-dir-path resolves to the same git directory and commit as the source repo at ref {}; cross-branch analysis is a no-op",
                            args.target_ref
                        );
                    }
                }
            }
        }
    }

    Ok(Some(TargetContext {
        index,
        translations,
        walk_limit: args.target_walk_limit,
        already_present_enabled: !args.skip_already_present,
    }))
}

fn same_repo(a: &Path, b: &Path) -> bool {
    a.canonicalize()
        .ok()
        .zip(b.canonicalize().ok())
        .is_some_and(|(x, y)| x == y)
}

/// Per-touched-path summary across the patch under analysis.
#[derive(Debug)]
pub struct TouchedPathSummary {
    pub renames: Vec<(PathBuf, PathBuf, TranslationSource)>,
    pub missing: Vec<PathBuf>,
    pub on_target: usize,
}

pub fn classify_touched_paths(
    touched: &[TouchedPath<'_>],
    ctx: &TargetContext,
) -> TouchedPathSummary {
    let mut renames = Vec::new();
    let mut missing = Vec::new();
    let mut on_target = 0usize;
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for tp in touched {
        let normalised = match normalise(tp.path) {
            Some(n) => n,
            None => continue,
        };
        if !seen.insert(normalised.clone()) {
            continue;
        }
        let q = TouchedPathQuery {
            path: tp.path,
            is_deletion: tp.is_deletion,
        };
        match classify_path(q, &ctx.index, &ctx.translations) {
            TargetPathClassification::OnTarget => on_target += 1,
            TargetPathClassification::TranslatesTo {
                target_path,
                translation,
            } => renames.push((normalised, target_path, translation)),
            TargetPathClassification::MissingFromTarget => missing.push(normalised),
            TargetPathClassification::DeletedByPatch | TargetPathClassification::UnsafePath => {}
        }
    }
    TouchedPathSummary {
        renames,
        missing,
        on_target,
    }
}

#[derive(Debug)]
pub struct TouchedPath<'a> {
    pub path: &'a Path,
    pub is_deletion: bool,
}

/// Merge cross-branch findings into every pin's verdict. Renames
/// produce `PathRename` reasons (RequiresAdaptation when present
/// alone). Missing paths join the existing verdict shape or, when
/// the pin had no other signal, become `Inapplicable::PathsMissingOnTarget`.
pub fn merge_into_series_verdict(summary: &TouchedPathSummary, verdict: &mut SeriesVerdict) {
    if summary.renames.is_empty() && summary.missing.is_empty() {
        return;
    }
    for pin in verdict.results.iter_mut() {
        let mut extra_reasons: Vec<Reason> = summary
            .renames
            .iter()
            .map(|(s, t, tr)| Reason::PathRename {
                source_path: s.clone(),
                target_path: t.clone(),
                translation: tr.clone(),
            })
            .collect();
        match &mut pin.verdict {
            Verdict::Compatible => {
                if extra_reasons.is_empty() && !summary.missing.is_empty() && summary.on_target == 0
                {
                    let rels: Vec<RelativePath> = summary
                        .missing
                        .iter()
                        .filter_map(|p| {
                            p.to_str()
                                .and_then(|s| RelativePath::new(s.to_owned()).ok())
                        })
                        .collect();
                    pin.verdict = Verdict::Inapplicable {
                        reason: InapplicableReason::PathsMissingOnTarget { paths: rels },
                    };
                } else if !extra_reasons.is_empty() {
                    pin.verdict = Verdict::from_reasons(extra_reasons);
                }
            }
            Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
                reasons.append(&mut extra_reasons);
            }
            Verdict::Inapplicable { .. } => {
                if !extra_reasons.is_empty() {
                    pin.verdict = Verdict::from_reasons(extra_reasons);
                }
            }
        }
    }
    verdict.summary = recount_summary(&verdict.results);
}

fn recount_summary(results: &[PinVerdict]) -> SeriesSummary {
    let mut s = SeriesSummary::default();
    for r in results {
        match &r.verdict {
            Verdict::Compatible => s.compatible += 1,
            Verdict::RequiresAdaptation { .. } => s.requires_adaptation += 1,
            Verdict::Incompatible { .. } => s.incompatible += 1,
            Verdict::Inapplicable { .. } => s.inapplicable += 1,
        }
    }
    s
}

/// Synthesise the post-patch content of `file` from its `Added`
/// hunk lines. Returns `None` when the file is not a fully-added
/// `.erl` or `.hrl` (the only shape the v1 018 resolvers handle).
pub fn synthesise_added_file_content(file: &PatchedFile) -> Option<(RelativePath, String)> {
    if file.binary || file.old_path.is_some() {
        return None;
    }
    let new_path = file.new_path.as_ref()?;
    if new_path == Path::new("/dev/null") {
        return None;
    }
    if !is_erl_or_hrl(new_path) {
        return None;
    }
    let path_str = new_path.to_str()?.to_owned();
    let path = RelativePath::new(path_str).ok()?;
    let mut content = String::new();
    let mut saw_added = false;
    for hunk in &file.hunks {
        for line in &hunk.lines {
            if let HunkLine::Added(s) = line {
                content.push_str(s);
                content.push('\n');
                saw_added = true;
            }
        }
    }
    if !saw_added {
        return None;
    }
    Some((path, content))
}

fn is_erl_or_hrl(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("erl") || s.eq_ignore_ascii_case("hrl"))
}

/// Union of every configured project family's
/// `test_helper_search_paths`. The 018 resolvers cast a wide net by
/// design: the user can pass the same patch through different
/// projects without re-declaring globs per call.
pub fn collect_search_path_globs(cfg: &Config) -> Vec<String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for project in &cfg.projects {
        for glob in project.family.defaults().test_helper_search_paths {
            seen.insert(glob);
        }
    }
    seen.into_iter().collect()
}

/// Walk every added `.erl` and `.hrl` in `files`, run the 018
/// resolvers (test-helper missing, behaviour missing, header
/// missing), and return the merged findings.
pub fn collect_added_file_findings(
    files: &[PatchedFile],
    target: &TargetTreeIndex,
    search_path_globs: &[String],
) -> AddedFileFindings {
    let added: Vec<(RelativePath, String)> = files
        .iter()
        .filter_map(synthesise_added_file_content)
        .collect();
    analyse_added_files(
        added.iter().map(|(p, c)| (p, c.as_str())),
        target,
        search_path_globs,
    )
}

/// Added-line text of every touched `.erl` or `.hrl` file, modified
/// files included: a macro or record reference dangles on a
/// cleanly-modified file too, so this does not require `old_path` to be
/// absent.
fn collect_define_subject_text(files: &[PatchedFile]) -> Vec<(RelativePath, String)> {
    let mut out = Vec::new();
    for file in files {
        if file.binary {
            continue;
        }
        let Some(new_path) = file.new_path.as_ref() else {
            continue;
        };
        if new_path == Path::new("/dev/null") || !is_erl_or_hrl(new_path) {
            continue;
        }
        let Some(path) = new_path.to_str().and_then(|s| RelativePath::new(s).ok()) else {
            continue;
        };
        let mut added = String::new();
        for hunk in &file.hunks {
            for line in &hunk.lines {
                if let HunkLine::Added(s) = line {
                    added.push_str(s);
                    added.push('\n');
                }
            }
        }
        if !added.is_empty() {
            out.push((path, added));
        }
    }
    out
}

/// Resolve `?MACRO` and `#record` uses the patch adds against the
/// target tree, reading target blobs through `repo` at the indexed
/// commit (the path as-is, then any configured translation).
pub fn collect_define_symbol_findings(files: &[PatchedFile], ctx: &TargetContext) -> Vec<Reason> {
    let subjects_text = collect_define_subject_text(files);
    if subjects_text.is_empty() {
        return Vec::new();
    }
    let Ok(repo) = GitRepo::open(ctx.index.target_repo().to_path_buf()) else {
        return Vec::new();
    };
    let commit = ctx.index.resolved_commit().clone();
    let read_target = |path: &RelativePath| -> Option<String> {
        read_target_text(&repo, &commit, &ctx.translations, path)
    };
    let subjects: Vec<DefineSubject<'_>> = subjects_text
        .iter()
        .map(|(p, t)| DefineSubject {
            source_path: p,
            added_text: t,
        })
        .collect();
    analyse_define_symbols(&subjects, &ctx.index, &read_target)
}

/// Resolve unqualified calls the patch adds against the target
/// module's function set. `.erl` files only: a `.hrl` defines no
/// function set of its own.
pub fn collect_local_call_findings(files: &[PatchedFile], ctx: &TargetContext) -> Vec<Reason> {
    let subjects_text: Vec<(RelativePath, String)> = collect_define_subject_text(files)
        .into_iter()
        .filter(|(p, _)| p.as_str().ends_with(".erl"))
        .collect();
    if subjects_text.is_empty() {
        return Vec::new();
    }
    let Ok(repo) = GitRepo::open(ctx.index.target_repo().to_path_buf()) else {
        return Vec::new();
    };
    let commit = ctx.index.resolved_commit().clone();
    let read_target = |path: &RelativePath| -> Option<String> {
        read_target_text(&repo, &commit, &ctx.translations, path)
    };
    let subjects: Vec<LocalCallSubject<'_>> = subjects_text
        .iter()
        .map(|(p, t)| LocalCallSubject {
            source_path: p,
            added_text: t,
        })
        .collect();
    analyse_local_calls(&subjects, &read_target)
}

/// Classify every touched file's hunks against the target tree:
/// `Missing` predicts a textual conflict, `Drifted` a clean shifted
/// apply.
pub fn collect_target_preimage_findings(files: &[PatchedFile], ctx: &TargetContext) -> Vec<Reason> {
    let Ok(repo) = GitRepo::open(ctx.index.target_repo().to_path_buf()) else {
        return Vec::new();
    };
    let commit = ctx.index.resolved_commit().clone();
    let mut reasons = Vec::new();
    for file in files {
        if file.binary {
            continue;
        }
        let Some(path) = file.new_path.as_deref().or(file.old_path.as_deref()) else {
            continue;
        };
        if path == Path::new("/dev/null") {
            continue;
        }
        let Some(rel) = path
            .to_str()
            .and_then(|s| RelativePath::new(s.to_owned()).ok())
        else {
            continue;
        };
        if let Some(content) = read_target_text(&repo, &commit, &ctx.translations, &rel) {
            reasons.extend(classify_hunks_against_target(path, &file.hunks, &content));
        }
    }
    reasons
}

/// Read a path's bytes at `commit`, falling back to its translated
/// path so a relocated layout still resolves. `None` when neither
/// exists or the blob is not valid UTF-8.
fn read_target_text(
    repo: &GitRepo,
    commit: &CommitSha,
    translations: &PathTranslations,
    path: &RelativePath,
) -> Option<String> {
    let direct = repo
        .read_blob_at(commit, Path::new(path.as_str()))
        .ok()
        .flatten();
    let bytes = direct.or_else(|| {
        let (translated, _) = translations.translate(path.as_str())?;
        repo.read_blob_at(commit, &translated).ok().flatten()
    })?;
    String::from_utf8(bytes).ok()
}

/// Merge `findings` into every pin's verdict and the series-level
/// diagnostics. Non-blocking reasons promote `Compatible` to
/// `RequiresAdaptation` and append to existing reason vectors;
/// `Inapplicable` rows are left alone (the user's reason for
/// "this pin has nothing to say" already trumps a new advisory).
pub fn merge_added_file_findings_into_evaluation(
    findings: AddedFileFindings,
    evaluation: &mut SeriesEvaluation,
) {
    let AddedFileFindings {
        reasons,
        missing_test_modules,
        ..
    } = findings;
    if reasons.is_empty() && missing_test_modules.is_empty() {
        return;
    }
    merge_reasons_into_evaluation(reasons, evaluation);
    merge_missing_test_modules(&mut evaluation.diagnostics, missing_test_modules);
}

/// Append non-blocking reasons to every pin: a `Compatible` pin
/// becomes `RequiresAdaptation`, an existing reason vector grows,
/// `Inapplicable` is left alone.
pub fn merge_reasons_into_evaluation(reasons: Vec<Reason>, evaluation: &mut SeriesEvaluation) {
    if reasons.is_empty() {
        return;
    }
    for pin in evaluation.verdict.results.iter_mut() {
        let mut to_add = reasons.clone();
        match &mut pin.verdict {
            Verdict::Compatible => pin.verdict = Verdict::from_reasons(to_add),
            Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
                reasons.append(&mut to_add);
            }
            Verdict::Inapplicable { .. } => {}
        }
    }
    evaluation.verdict.summary = recount_summary(&evaluation.verdict.results);
}

fn merge_missing_test_modules(
    diagnostics: &mut Diagnostics,
    incoming: BTreeMap<RelativePath, BTreeMap<ModuleName, usize>>,
) {
    for (suite, by_mod) in incoming {
        let entry = diagnostics.missing_test_modules.entry(suite).or_default();
        for (m, count) in by_mod {
            *entry.entry(m).or_insert(0) += count;
        }
    }
}
