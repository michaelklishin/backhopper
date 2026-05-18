use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::str;

use bel7_cli::{
    InteractiveReporter, NonInteractiveReporter, ProgressReporter, QuietReporter, TableStyle,
};
use serde::Serialize;

use backhopper_core::Error as CoreError;
use backhopper_core::compat::is_otp_module;
use backhopper_core::compat::patch::{EvaluationContext, EvaluationFiles, Patch};
use backhopper_core::compat::scope::{PinScope, parse_module_names};
use backhopper_core::compat::source_macros::{FileMap, build_macro_table};
use backhopper_core::config::{Config, Project};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::errors::GitError;
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::model::verdict::{
    Diagnostics, PinVerdict, Reason, SeriesEvaluation, SeriesVerdict, Verdict,
};
use backhopper_core::store::{ReadOnly, SnapshotStore};

use crate::cli::{CheckCmd, CheckOutputFlags, Formatter, GlobalArgs, SourcePinArgs};
use crate::commands::context::{load_config, open_store_read};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};
use crate::tables::render_evaluation_table;

#[derive(Debug, Serialize)]
struct CompatPayload {
    queried_against: QueriedAgainst,
    results: SeriesVerdict,
    #[serde(skip_serializing_if = "Diagnostics::is_empty")]
    diagnostics: Diagnostics,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum QueriedAgainst {
    Pin { project: String, tag: String },
    Series { name: String, pins: Vec<PinPayload> },
}

#[derive(Debug, Serialize)]
struct PinPayload {
    project: String,
    tag: String,
}

pub fn handle(args: &GlobalArgs, cmd: CheckCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        CheckCmd::Patch {
            project,
            tag,
            series,
            explain: _,
            repo_dir_path,
            source,
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
                repo_dir_path.as_deref(),
                &FileMap::new(),
                &source,
                diagnostics,
            )
        }
        CheckCmd::Commit {
            project,
            tag,
            series,
            repo_dir_path,
            source,
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
                diagnostics,
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
                diagnostics,
            )
        }
        CheckCmd::Batch {
            series,
            repo_dir_path,
            commits_file_path,
            source,
            diagnostics,
        } => run_batch(
            args,
            &cfg,
            &series,
            &repo_dir_path,
            &commits_file_path,
            &source,
            diagnostics,
        ),
    }
}

fn build_pin_files(
    project: &Project,
    pin: &Pin,
    scope: &PinScope,
    touched_paths: &[PathBuf],
) -> CliResult<EvaluationFiles> {
    let in_scope: Vec<(PathBuf, PathBuf)> = touched_paths
        .iter()
        .filter_map(|p| scope.rewrite_path(p).map(|r| (p.clone(), r.to_path_buf())))
        .collect();
    if in_scope.is_empty() {
        return Ok(EvaluationFiles::new());
    }
    let repo = GitRepo::open(project.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let commit = repo
        .resolve_tag(&pin.tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let needed: BTreeSet<String> = in_scope
        .iter()
        .map(|(_, project_path)| project_path.to_string_lossy().into_owned())
        .collect();
    let blobs = repo
        .read_paths_at_commit(&commit, |p| needed.contains(p))
        .map_err(|e| CliError::Core(e.into()))?;
    let present: BTreeMap<String, Vec<u8>> = blobs
        .into_iter()
        .map(|b| (b.path.to_string_lossy().into_owned(), b.bytes))
        .collect();
    let mut files = EvaluationFiles::new();
    for (original_path, project_path) in &in_scope {
        let key = project_path.to_string_lossy().into_owned();
        let contents = present.get(&key).cloned();
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
        updated_results
            .push(PinVerdict::new(pv.pin, new_verdict).with_tracked_refs(pv.tracked_refs));
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
            let name_str = name.to_string_lossy().to_string();
            if let Some(stem) = name_str.strip_suffix(".erl") {
                out.insert(stem.to_string());
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

fn commit_patch_bytes(repo: &Path, commit: &str) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let to = resolve_rev_with_hint(&g, repo, commit)?;
    let from = g
        .parent_commit(&to)
        .map_err(|e| CliError::Core(e.into()))?
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
    diagnostics: CheckOutputFlags,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let pins: Vec<Pin> = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => vec![Pin::new(p.clone(), t.clone())],
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
    let source_pins = resolve_source_pins(
        cfg,
        &pins,
        project.as_ref(),
        tag.as_ref(),
        series.as_ref(),
        source,
    )?;
    let mut evaluation = evaluate_one(cfg, &store, bytes, &pins, &source_pins, source_files)?;
    if diagnostics.resolve_untracked_modules {
        let repo = repo_dir_path.ok_or_else(|| {
            CliError::InvalidInput("--resolve-untracked-modules requires --repo-dir-path".into())
        })?;
        resolve_untracked_modules_against_tree(&mut evaluation, repo)?;
    }
    let queried = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => QueriedAgainst::Pin {
            project: p.to_string(),
            tag: t.to_string(),
        },
        (None, None, Some(s)) => QueriedAgainst::Series {
            name: s.to_string(),
            pins: pins
                .iter()
                .map(|p| PinPayload {
                    project: p.project.to_string(),
                    tag: p.tag.to_string(),
                })
                .collect(),
        },
        _ => unreachable!(),
    };
    let payload = CompatPayload {
        queried_against: queried,
        results: evaluation.verdict.clone(),
        diagnostics: evaluation.diagnostics.clone(),
    };
    let known_projects: Vec<ProjectName> = cfg.projects.iter().map(|p| p.name.clone()).collect();
    let ctx = OutputContext::new(args.formatter, "check patch");
    let exit = evaluation.worst_exit_code();
    let style = args.table_style;
    render_with_exit(&ctx, &payload, exit, |w| {
        render_text(w, &evaluation, &known_projects, diagnostics, style)
    })
}

fn render_text(
    w: &mut dyn Write,
    evaluation: &SeriesEvaluation,
    known_projects: &[ProjectName],
    flags: CheckOutputFlags,
    style: TableStyle,
) -> CliResult<()> {
    let v = &evaluation.verdict;
    if flags.summary_only {
        writeln!(
            w,
            "compatible={} requires_adaptation={} incompatible={}",
            v.summary.compatible, v.summary.requires_adaptation, v.summary.incompatible,
        )?;
        return Ok(());
    }
    writeln!(
        w,
        "compatible: {}, requires_adaptation: {}, incompatible: {}",
        v.summary.compatible, v.summary.requires_adaptation, v.summary.incompatible,
    )?;
    writeln!(w)?;
    writeln!(w, "{}", render_evaluation_table(evaluation, style))?;
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
            writeln!(w, "  #{:<39} {} reference{}", record, count, plural)?;
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
                "  {:<40} {} (apply/spawn/spawn_link/spawn_monitor/spawn_opt/hibernate)",
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

#[derive(Debug, Serialize)]
struct BatchPayload {
    queried_against: Vec<BatchQuery>,
    results: Vec<BatchResult>,
}

#[derive(Debug, Serialize)]
struct BatchQuery {
    series: String,
    pins: Vec<PinPayload>,
}

#[derive(Debug, Serialize)]
struct BatchResult {
    commit: String,
    series: String,
    verdict: SeriesVerdict,
    #[serde(skip_serializing_if = "Diagnostics::is_empty")]
    diagnostics: Diagnostics,
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

fn evaluate_one(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    bytes: &[u8],
    pins: &[Pin],
    source_pins: &[Option<Pin>],
    source_files: &FileMap,
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
    let mut contexts = Vec::with_capacity(pins.len());
    for (idx, pin) in pins.iter().enumerate() {
        let project = cfg
            .project(&pin.project)
            .map_err(|e| CliError::Core(e.into()))?;
        let snap = store
            .read(&pin.project, &pin.tag)
            .map_err(|e| CliError::Core(e.into()))?;
        let scope = build_pin_scope(project, &snap);
        let files = build_pin_files(project, pin, &scope, &touched_paths)?;
        let mut ctx = EvaluationContext::new(pin.clone(), snap, scope).with_files(files);
        if let Some(Some(source_pin)) = source_pins.get(idx) {
            let source_snap = store
                .read(&source_pin.project, &source_pin.tag)
                .map_err(|e| CliError::Core(e.into()))?;
            ctx = ctx.with_source_snapshot(source_snap);
        }
        contexts.push(ctx);
    }
    Ok(analyzed.evaluate_series(&contexts))
}

fn resolve_source_pins(
    cfg: &Config,
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
            let mut by_project: BTreeMap<&ProjectName, &TagName> = BTreeMap::new();
            for pin in &src_series.pins {
                by_project.insert(&pin.project, &pin.tag);
            }
            let mut out = Vec::with_capacity(target_pins.len());
            for target_pin in target_pins {
                let mapped = by_project
                    .get(&target_pin.project)
                    .map(|tag| Pin::new(target_pin.project.clone(), (*tag).clone()));
                out.push(mapped);
            }
            Ok(out)
        }
        _ => Err(CliError::InvalidInput(
            "--source-tag pairs with --project + --tag; --source-series pairs with --series".into(),
        )),
    }
}

fn run_batch(
    args: &GlobalArgs,
    cfg: &Config,
    series_names: &[SeriesName],
    repo: &Path,
    commits_file_path: &Path,
    source: &SourcePinArgs,
    diagnostics: CheckOutputFlags,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let commits = read_commits_file(commits_file_path)?;
    if commits.is_empty() {
        return Err(CliError::InvalidInput(
            "no commits supplied (empty file or stdin)".into(),
        ));
    }
    let mut resolved_series: Vec<(SeriesName, Vec<Pin>)> = Vec::with_capacity(series_names.len());
    for name in series_names {
        let s = cfg
            .series_by_name_with_coverage_check(name)
            .map_err(|e| CliError::Core(e.into()))?;
        resolved_series.push((name.clone(), s.pins.clone()));
    }
    let queried: Vec<BatchQuery> = resolved_series
        .iter()
        .map(|(name, pins)| BatchQuery {
            series: name.to_string(),
            pins: pins
                .iter()
                .map(|p| PinPayload {
                    project: p.project.to_string(),
                    tag: p.tag.to_string(),
                })
                .collect(),
        })
        .collect();
    let pair_count = commits.len() * resolved_series.len();
    let mut reporter = select_batch_reporter(args);
    reporter.start(pair_count, "evaluating commits");
    let mut results: Vec<BatchResult> = Vec::with_capacity(pair_count);
    let mut worst_exit: i32 = 0;
    let mut current: usize = 0;
    for commit in &commits {
        let bytes = commit_patch_bytes(repo, commit)?;
        let source_files = load_source_files_at_parent(repo, commit)?;
        for (name, pins) in &resolved_series {
            let item_label = format!("{commit} @ {name}");
            let source_pins = resolve_source_pins(cfg, pins, None, None, Some(name), source)?;
            let evaluation = evaluate_one(cfg, &store, &bytes, pins, &source_pins, &source_files)?;
            current += 1;
            reporter.progress(current, pair_count, &item_label);
            worst_exit = worst_exit.max(evaluation.worst_exit_code());
            results.push(BatchResult {
                commit: commit.clone(),
                series: name.to_string(),
                verdict: evaluation.verdict.clone(),
                diagnostics: evaluation.diagnostics.clone(),
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
    flags: CheckOutputFlags,
) -> CliResult<()> {
    if flags.summary_only {
        for r in results {
            writeln!(
                w,
                "{} {} compatible={} requires_adaptation={} incompatible={}",
                r.commit,
                r.series,
                r.verdict.summary.compatible,
                r.verdict.summary.requires_adaptation,
                r.verdict.summary.incompatible,
            )?;
        }
        return Ok(());
    }
    let totals = results.iter().fold((0u32, 0u32, 0u32), |(c, r, i), x| {
        (
            c + x.verdict.summary.compatible,
            r + x.verdict.summary.requires_adaptation,
            i + x.verdict.summary.incompatible,
        )
    });
    writeln!(
        w,
        "totals: compatible={} requires_adaptation={} incompatible={}",
        totals.0, totals.1, totals.2,
    )?;
    writeln!(w)?;
    for r in results {
        writeln!(
            w,
            "{}  {:<20} compatible={} requires_adaptation={} incompatible={}",
            r.commit,
            r.series,
            r.verdict.summary.compatible,
            r.verdict.summary.requires_adaptation,
            r.verdict.summary.incompatible,
        )?;
    }
    Ok(())
}
