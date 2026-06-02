// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! CLI-side helpers for the `--target-repo-dir-path` cross-branch
//! analyser. Builds the `TargetTreeIndex` and merged translations
//! once per invocation, then applies the per-touched-path classifier
//! to every pin's verdict.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use backhopper_core::compat::{
    TargetPathClassification, TouchedPathQuery, classify_path, normalise,
};
use backhopper_core::config::PathTranslations;
use backhopper_core::git::{GitRepo, TargetTreeIndex};
use backhopper_core::model::names::GitRef;
use backhopper_core::model::verdict::{
    InapplicableReason, PinVerdict, Reason, SeriesSummary, SeriesVerdict, TranslationSource,
    Verdict,
};

use crate::cli::check::TargetRepoArgs;
use crate::errors::{CliError, CliResult};

#[derive(Debug)]
pub struct TargetContext {
    pub index: TargetTreeIndex,
    pub translations: PathTranslations,
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
    let repo = GitRepo::open(target_repo.clone()).map_err(|e| CliError::Core(e.into()))?;
    let target_ref = GitRef::new(args.target_ref.clone()).map_err(|e| CliError::Core(e.into()))?;
    let index = TargetTreeIndex::build(&repo, &target_ref).map_err(|e| CliError::Core(e.into()))?;

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
                    pin.verdict = Verdict::Inapplicable {
                        reason: InapplicableReason::PathsMissingOnTarget {
                            paths: summary.missing.clone(),
                        },
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
