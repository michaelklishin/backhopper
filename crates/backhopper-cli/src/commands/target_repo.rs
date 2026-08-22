// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! CLI-side helpers for the `--target-repo-dir-path` cross-branch
//! analyser. Builds the `TargetTreeIndex` and merged translations
//! once per invocation, then applies the per-touched-path classifier
//! to every pin's verdict.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use backhopper_core::compat::added_file::{AddedFileFindings, analyse_added_files};
use backhopper_core::compat::added_lines::{
    AddedLinesSubject, added_lines_with_context, added_lines_with_offsets,
};
use backhopper_core::compat::behaviour_callback_resolve::analyse_behaviour_callbacks;
use backhopper_core::compat::classify_hunks_against_target;
use backhopper_core::compat::define_resolve::{
    MacroValueAnalysis, analyse_define_symbols, analyse_macro_values,
};
use backhopper_core::compat::exported_type_resolve::analyse_exported_types;
use backhopper_core::compat::local_call_resolve::{LocalCallAnalysis, analyse_local_calls};
use backhopper_core::compat::patch::{HunkLine, PatchedFile};
use backhopper_core::compat::qualified_call_resolve::{
    ContextAwareSubject, IndirectCallAnalysis, QualifiedCallAnalysis, ReferenceCaches,
    ReferenceContext, analyse_indirect_elixir_calls, analyse_qualified_calls, patch_provided,
};
use backhopper_core::compat::target_tree_index::TargetTreeIndex;
use backhopper_core::compat::{
    TargetPathClassification, TouchedPathQuery, classify_path, normalise,
};
use backhopper_core::config::{Config, PathTranslations};
use backhopper_core::model::apply::{ApplyForecast, PathApplyOutcome, UnassessedReason};
use backhopper_core::model::names::{CommitSha, GitRef, ModuleName, RelativePath};
use backhopper_core::model::symbol::LineClass;
use backhopper_core::model::verdict::{
    ApplyConflictKind, Diagnostics, InapplicableReason, PinVerdict, Reason, SeriesEvaluation,
    SeriesSummary, SeriesVerdict, TranslationSource, Verdict,
};
use backhopper_git::{GitRepo, build_target_tree_index};

use crate::cli::check::TargetRepoArgs;
use crate::errors::CliResult;

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
        let external = PathTranslations::load_external(file)?;
        translations.merge_external(external)?;
    }
    let repo = GitRepo::open(target_repo.clone())?;
    let target_ref = GitRef::new(args.target_ref.clone())?;
    let index = build_target_tree_index(&repo, &target_ref)?;

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

/// One non-blocking `TargetPathAbsent` per absent path, empty unless
/// some path is also present (all-absent is the whole-patch
/// `PathsMissingOnTarget` case handled in `merge_into_series_verdict`).
pub fn partial_absence_reasons(summary: &TouchedPathSummary) -> Vec<Reason> {
    if summary.on_target == 0 {
        return Vec::new();
    }
    summary
        .missing
        .iter()
        .filter_map(|p| {
            p.to_str()
                .and_then(|s| RelativePath::new(s.to_owned()).ok())
        })
        .map(|path| Reason::TargetPathAbsent { path })
        .collect()
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
/// `.erl` or `.hrl` (the only shape the added-file resolvers handle).
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

/// Full text of every file the patch creates, keyed by path. Only pure
/// additions enter: for a modified file the added lines are not the
/// whole content, so the target text stays authoritative.
pub(crate) fn patch_added_file_map(files: &[PatchedFile]) -> BTreeMap<RelativePath, String> {
    files
        .iter()
        .filter_map(synthesise_added_file_content)
        .collect()
}

fn is_erl_or_hrl(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("erl") || s.eq_ignore_ascii_case("hrl"))
}

fn is_elixir(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("ex") || s.eq_ignore_ascii_case("exs"))
}

/// Union of every configured project family's
/// `test_helper_search_paths`. The added-file resolvers search broadly
/// by design: the user can pass the same patch through different
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

/// Walk every added `.erl` and `.hrl` in `files`, run the added-file
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

/// Added-line text of every touched `.erl` or `.hrl` file, with the
/// blob-line to file-line map for each. Modified files are included: a
/// macro or record reference dangles on a cleanly-modified file too, so
/// this does not require `old_path` to be absent.
fn collect_define_subject_text(files: &[PatchedFile]) -> Vec<(RelativePath, String, Vec<u32>)> {
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
        let (added, line_map) = added_lines_with_offsets(&file.hunks);
        if !added.is_empty() {
            out.push((path, added, line_map));
        }
    }
    out
}

/// `.erl`-only subject text: the axes keyed on a module's own function
/// set (a `.hrl` defines none).
fn erl_subject_text(files: &[PatchedFile]) -> Vec<(RelativePath, String, Vec<u32>)> {
    collect_define_subject_text(files)
        .into_iter()
        .filter(|(p, _, _)| p.as_str().ends_with(".erl"))
        .collect()
}

/// Borrow the subject-text tuples as the `AddedLinesSubject` shape the
/// resolvers take.
fn to_subjects(subjects_text: &[(RelativePath, String, Vec<u32>)]) -> Vec<AddedLinesSubject<'_>> {
    subjects_text
        .iter()
        .map(|(p, t, lm)| AddedLinesSubject {
            source_path: p,
            added_text: t,
            line_map: lm,
        })
        .collect()
}

/// Added-line text of every touched `.erl` file, with the blob-line to
/// file-line map and each blob line's attribute-region classification,
/// walked against the full hunk so an orphaned attribute continuation
/// still classifies correctly. The qualified-call, Erlang
/// indirect-call, and local-call axes consume the classification.
fn erl_subject_context(
    files: &[PatchedFile],
) -> Vec<(RelativePath, String, Vec<u32>, Vec<LineClass>)> {
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
        if !path.as_str().ends_with(".erl") {
            continue;
        }
        let (added, line_map, ctx) = added_lines_with_context(&file.hunks);
        if !added.is_empty() {
            out.push((path, added, line_map, ctx));
        }
    }
    out
}

/// Borrow the context-carrying subject tuples as the `ContextAwareSubject`
/// shape `analyse_qualified_calls` takes.
fn to_context_subjects(
    subjects: &[(RelativePath, String, Vec<u32>, Vec<LineClass>)],
) -> Vec<ContextAwareSubject<'_>> {
    subjects
        .iter()
        .map(|(p, t, lm, ctx)| ContextAwareSubject {
            subject: AddedLinesSubject {
                source_path: p,
                added_text: t,
                line_map: lm,
            },
            line_context: ctx,
        })
        .collect()
}

/// The Erlang module a `.erl` path defines, by basename.
fn module_of_erl_path(path: &RelativePath) -> Option<ModuleName> {
    let stem = Path::new(path.as_str()).file_stem()?.to_str()?;
    ModuleName::from_str(stem).ok()
}

/// One target-tree resolution context shared by every symbol axis.
/// Opens the target repo once and lends the `read_target` and
/// `resolve_module_path` closures each axis would otherwise rebuild,
/// so a batch row opens the git directory once instead of per axis.
pub(crate) struct TargetResolveSession<'a> {
    repo: GitRepo,
    commit: CommitSha,
    ctx: &'a TargetContext,
    source_repo_dir: Option<&'a Path>,
}

impl<'a> TargetResolveSession<'a> {
    /// `None` when the target repo cannot be opened, in which case
    /// every axis falls back to its empty default.
    pub(crate) fn open(ctx: &'a TargetContext, source_repo_dir: Option<&'a Path>) -> Option<Self> {
        let repo = GitRepo::open(ctx.index.target_repo().to_path_buf()).ok()?;
        let commit = ctx.index.resolved_commit().clone();
        Some(Self {
            repo,
            commit,
            ctx,
            source_repo_dir,
        })
    }

    fn read_target_text(&self, path: &RelativePath) -> Option<String> {
        read_target_text(&self.repo, &self.commit, &self.ctx.translations, path)
    }

    fn resolve_module_path(&self, m: &ModuleName) -> Option<RelativePath> {
        let path = self.ctx.index.module_erl_path(m)?;
        RelativePath::new(path.to_str()?.to_owned()).ok()
    }

    /// Resolve `?MACRO` and `#record` uses the patch adds against the
    /// target tree.
    pub(crate) fn define_symbol_findings(
        &self,
        files: &[PatchedFile],
        patch_added: &BTreeMap<RelativePath, String>,
    ) -> Vec<Reason> {
        let subjects_text = collect_define_subject_text(files);
        if subjects_text.is_empty() {
            return Vec::new();
        }
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let subjects = to_subjects(&subjects_text);
        analyse_define_symbols(&subjects, patch_added, &self.ctx.index, &read_target)
    }

    /// Resolve `-export_type` entries the patch adds against the type
    /// declarations the target module reaches. `.erl` files only.
    pub(crate) fn exported_type_findings(
        &self,
        files: &[PatchedFile],
        patch_added: &BTreeMap<RelativePath, String>,
    ) -> Vec<Reason> {
        let subjects_text = erl_subject_text(files);
        if subjects_text.is_empty() {
            return Vec::new();
        }
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let subjects = to_subjects(&subjects_text);
        analyse_exported_types(&subjects, patch_added, &self.ctx.index, &read_target)
    }

    /// Resolve unqualified calls the patch adds against the target
    /// module's function set, and compare resolved calls' `-spec`
    /// return shapes between the checkouts. `.erl` files only.
    pub(crate) fn local_call_findings(
        &self,
        files: &[PatchedFile],
        covered_modules: &BTreeSet<ModuleName>,
    ) -> LocalCallAnalysis {
        let subjects_ctx = erl_subject_context(files);
        if subjects_ctx.is_empty() {
            return LocalCallAnalysis::default();
        }
        let per_file: Vec<(ModuleName, &str)> = subjects_ctx
            .iter()
            .filter_map(|(p, t, _, _)| Some((module_of_erl_path(p)?, t.as_str())))
            .collect();
        let patch_added = patch_provided(&per_file);
        let resolve_module_path = |m: &ModuleName| self.resolve_module_path(m);
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let read_source_fn = self.source_repo_dir.map(|dir| {
            move |path: &RelativePath| -> Option<String> { read_source_text(dir, path) }
        });
        let read_source = read_source_fn
            .as_ref()
            .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
        let subjects = to_context_subjects(&subjects_ctx);
        let reference_ctx = ReferenceContext {
            covered_modules,
            patch_added: &patch_added,
            resolve_module_path: &resolve_module_path,
            read_target: &read_target,
            read_source,
        };
        analyse_local_calls(&subjects, &reference_ctx)
    }

    /// Resolve qualified `m:f/a` calls the patch adds against the
    /// called module's exports on the target tree, and compare resolved
    /// calls' `-spec` return shapes between the checkouts. `.erl` files
    /// only.
    pub(crate) fn qualified_call_findings(
        &self,
        files: &[PatchedFile],
        covered_modules: &BTreeSet<ModuleName>,
    ) -> QualifiedCallAnalysis {
        let subjects_ctx = erl_subject_context(files);
        if subjects_ctx.is_empty() {
            return QualifiedCallAnalysis::default();
        }
        let per_file: Vec<(ModuleName, &str)> = subjects_ctx
            .iter()
            .filter_map(|(p, t, _, _)| Some((module_of_erl_path(p)?, t.as_str())))
            .collect();
        let patch_added = patch_provided(&per_file);
        let resolve_module_path = |m: &ModuleName| self.resolve_module_path(m);
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let read_source_fn = self.source_repo_dir.map(|dir| {
            move |path: &RelativePath| -> Option<String> { read_source_text(dir, path) }
        });
        let read_source = read_source_fn
            .as_ref()
            .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
        let subjects = to_context_subjects(&subjects_ctx);
        analyse_qualified_calls(
            &subjects,
            covered_modules,
            &patch_added,
            &resolve_module_path,
            &read_target,
            read_source,
        )
    }

    /// Resolve the `m:f/a` an Elixir call carries to a recognized rpc
    /// form against the Erlang target tree. `.ex` and `.exs` files only.
    /// A function the same commit adds in an `.erl` file is not flagged.
    pub(crate) fn indirect_elixir_findings(
        &self,
        files: &[PatchedFile],
        covered_modules: &BTreeSet<ModuleName>,
    ) -> IndirectCallAnalysis {
        let subjects_text: Vec<(RelativePath, String, Vec<u32>)> = files
            .iter()
            .filter(|f| !f.binary)
            .filter_map(|f| {
                let new_path = f.new_path.as_ref()?;
                if new_path == Path::new("/dev/null") || !is_elixir(new_path) {
                    return None;
                }
                let path = new_path.to_str().and_then(|s| RelativePath::new(s).ok())?;
                let (added, line_map) = added_lines_with_offsets(&f.hunks);
                (!added.is_empty()).then_some((path, added, line_map))
            })
            .collect();
        if subjects_text.is_empty() {
            return IndirectCallAnalysis::default();
        }
        let erl_subjects = erl_subject_text(files);
        let erl_per_file: Vec<(ModuleName, &str)> = erl_subjects
            .iter()
            .filter_map(|(p, t, _)| Some((module_of_erl_path(p)?, t.as_str())))
            .collect();
        let patch_added = patch_provided(&erl_per_file);
        let resolve_module_path = |m: &ModuleName| self.resolve_module_path(m);
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let subjects = to_subjects(&subjects_text);
        let ctx = ReferenceContext {
            covered_modules,
            patch_added: &patch_added,
            resolve_module_path: &resolve_module_path,
            read_target: &read_target,
            read_source: None,
        };
        let mut caches = ReferenceCaches::default();
        analyse_indirect_elixir_calls(&subjects, &ctx, &mut caches)
    }

    /// Compare a declared behaviour's `-callback` surface between the
    /// two trees for behaviours the patch's added lines declare. `.erl`
    /// files only.
    pub(crate) fn behaviour_callback_findings(
        &self,
        files: &[PatchedFile],
        covered_modules: &BTreeSet<ModuleName>,
    ) -> Vec<Reason> {
        let subjects_text = erl_subject_text(files);
        if subjects_text.is_empty() {
            return Vec::new();
        }
        let resolve_module_path = |m: &ModuleName| self.resolve_module_path(m);
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let read_source_fn = self.source_repo_dir.map(|dir| {
            move |path: &RelativePath| -> Option<String> { read_source_text(dir, path) }
        });
        let read_source = read_source_fn
            .as_ref()
            .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
        let subjects = to_subjects(&subjects_text);
        analyse_behaviour_callbacks(
            &subjects,
            covered_modules,
            &resolve_module_path,
            &read_target,
            read_source,
        )
    }

    /// Compare same-file `-define` values for macros the patch uses
    /// between the two trees. `.erl` and `.hrl` alike: a define can
    /// appear in either.
    pub(crate) fn macro_value_findings(&self, files: &[PatchedFile]) -> MacroValueAnalysis {
        let subjects_text = collect_define_subject_text(files);
        if subjects_text.is_empty() {
            return MacroValueAnalysis::default();
        }
        let read_target = |path: &RelativePath| self.read_target_text(path);
        let read_source_fn = self.source_repo_dir.map(|dir| {
            move |path: &RelativePath| -> Option<String> { read_source_text(dir, path) }
        });
        let read_source = read_source_fn
            .as_ref()
            .map(|f| f as &dyn Fn(&RelativePath) -> Option<String>);
        let subjects = to_subjects(&subjects_text);
        analyse_macro_values(&subjects, &read_target, read_source)
    }

    /// Classify every touched file's hunks against the target tree.
    /// `Missing` predicts a textual conflict, `Drifted` a clean shifted
    /// apply. Each path also lands in the forecast: an absent path is a
    /// `FileAbsent` conflict, a binary or non-text path is stated
    /// unassessed, and a fully clean file is recorded rather than skipped.
    pub(crate) fn target_apply_analysis(&self, files: &[PatchedFile]) -> TargetApplyAnalysis {
        let mut analysis = TargetApplyAnalysis {
            reasons: Vec::new(),
            forecast: ApplyForecast::default(),
        };
        for file in files {
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
            if file.binary {
                analysis.forecast.record(
                    rel,
                    PathApplyOutcome::Unassessed {
                        reason: UnassessedReason::BinaryFile,
                    },
                );
                continue;
            }
            let outcome = match read_target(&self.repo, &self.commit, &self.ctx.translations, &rel)
            {
                TargetRead::Text(content) => {
                    let file_reasons = classify_hunks_against_target(path, &file.hunks, &content);
                    let outcome = path_outcome(&file_reasons);
                    analysis.reasons.extend(file_reasons);
                    outcome
                }
                TargetRead::NotText => PathApplyOutcome::Unassessed {
                    reason: UnassessedReason::TargetNotText,
                },
                TargetRead::Absent => PathApplyOutcome::Conflict {
                    kind: ApplyConflictKind::FileAbsent,
                },
            };
            analysis.forecast.record(rel, outcome);
        }
        analysis
    }
}

/// The source side reads the live checkout through the filesystem: no
/// single source commit exists for patch, range, and PR inputs.
fn read_source_text(repo_dir: &Path, path: &RelativePath) -> Option<String> {
    fs::read_to_string(repo_dir.join(path.as_str())).ok()
}

/// The target-tree apply pass: unchanged per-pin reasons plus the
/// per-path forecast, kept even for inapplicable verdicts.
#[derive(Debug)]
pub struct TargetApplyAnalysis {
    pub reasons: Vec<Reason>,
    pub forecast: ApplyForecast,
}

/// Fold one file's hunk classifications into its path outcome: the
/// worst conflict wins, else the largest-magnitude drift, else exact.
fn path_outcome(reasons: &[Reason]) -> PathApplyOutcome {
    let mut outcome = PathApplyOutcome::CleanExact;
    for reason in reasons {
        let next = match reason {
            Reason::PreimageDrifted { line_delta, .. } => PathApplyOutcome::CleanDrifted {
                line_delta: *line_delta as i64,
            },
            Reason::PreimageMissing { .. } => PathApplyOutcome::Conflict {
                kind: ApplyConflictKind::PreimageMissing,
            },
            Reason::PostimageCollision { .. } => PathApplyOutcome::Conflict {
                kind: ApplyConflictKind::PostimageCollision,
            },
            _ => continue,
        };
        outcome = outcome.merge(next);
    }
    outcome
}

/// What reading a path off the target tree found.
enum TargetRead {
    Text(String),
    /// The blob exists but is not valid UTF-8.
    NotText,
    Absent,
}

/// Read a path's bytes at `commit`, falling back to its translated
/// path so a relocated layout still resolves.
fn read_target(
    repo: &GitRepo,
    commit: &CommitSha,
    translations: &PathTranslations,
    path: &RelativePath,
) -> TargetRead {
    let direct = repo
        .read_blob_at(commit, Path::new(path.as_str()))
        .ok()
        .flatten();
    let bytes = direct.or_else(|| {
        let (translated, _) = translations.translate(path.as_str())?;
        repo.read_blob_at(commit, &translated).ok().flatten()
    });
    match bytes {
        None => TargetRead::Absent,
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(text) => TargetRead::Text(text),
            Err(_) => TargetRead::NotText,
        },
    }
}

fn read_target_text(
    repo: &GitRepo,
    commit: &CommitSha,
    translations: &PathTranslations,
    path: &RelativePath,
) -> Option<String> {
    match read_target(repo, commit, translations, path) {
        TargetRead::Text(text) => Some(text),
        TargetRead::NotText | TargetRead::Absent => None,
    }
}

/// Merge `findings` into every pin's verdict and the series-level
/// diagnostics. Non-blocking reasons promote `Compatible` to
/// `RequiresAdaptation` and append to existing reason vectors;
/// `Inapplicable` rows are left alone (the user's reason for
/// "this pin has nothing to say" already takes precedence over a new
/// advisory).
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
