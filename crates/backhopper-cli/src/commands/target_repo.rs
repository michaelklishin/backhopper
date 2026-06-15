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
use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::compat::{
    TargetPathClassification, TouchedPathQuery, classify_path, normalise,
};
use backhopper_core::config::{Config, PathTranslations};
use backhopper_core::model::names::{GitRef, ModuleName, RelativePath};
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
    if !reasons.is_empty() {
        for pin in evaluation.verdict.results.iter_mut() {
            let mut to_add = reasons.clone();
            match &mut pin.verdict {
                Verdict::Compatible => {
                    pin.verdict = Verdict::from_reasons(to_add);
                }
                Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
                    reasons.append(&mut to_add);
                }
                Verdict::Inapplicable { .. } => {}
            }
        }
        evaluation.verdict.summary = recount_summary(&evaluation.verdict.results);
    }
    merge_missing_test_modules(&mut evaluation.diagnostics, missing_test_modules);
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
