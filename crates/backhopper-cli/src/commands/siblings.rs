// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `siblings doctor`: rank sibling-branch commits that should have
//! cascaded to a series but never did.
//!
//! Pipeline, in cost order: first-parent walk of each source branch,
//! vocabulary subject prefilter, first-parent diff plus target-tree
//! path identity, already-cascaded suppression (`-x` trailers plus
//! patch-id equivalence against the target window), then score and
//! rank. Nothing here mutates a repository.

use crate::outcome::CommandOutcome;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tabled::Tabled;
use time::OffsetDateTime;

use backhopper_cache::{CacheDir, CacheMode, content_hash, ttl_from_days};
use backhopper_core::compat::patch::{HunkLine, Patch};
use backhopper_core::config::{Config, Project, Series};
use backhopper_core::model::names::{CommitSha, CommitShaPrefix, GitRef, RelativePath, TagName};
use backhopper_core::model::pin::PinSpec;
use backhopper_core::model::sibling_drift::{
    ActionThresholds, ScoreWeights, Scored, SiblingCandidate, SiblingDoctorReport, SinceDerivation,
    Unscored, Vocabulary, VocabularySource,
};
use backhopper_git::walk::{WalkedCommit, cherry_pick_trailers, first_parent_walk_since, patch_id};
use backhopper_git::{
    GitRepo, MergePolicy, PatchId, PrCommitPolicy, ResolvedPatchInput, build_target_tree_index,
};

use crate::cli::{Formatter, GlobalArgs, SiblingsCmd, SiblingsDoctorArgs};
use crate::commands::context::{load_config, snapshot_dir};
use crate::commands::self_snapshot::effective_self_repo;
use crate::commands::sha_prefix::expand_prefix_enriched;
use crate::commands::summary::SummaryFormatter;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, emit_jsonl, render_with_exit};
use crate::tables::styled_table;

const CACHE_DIR_NAME: &str = ".siblings_doctor_cache";

pub fn handle(args: &GlobalArgs, cmd: SiblingsCmd) -> CliResult<CommandOutcome> {
    match cmd {
        SiblingsCmd::Doctor(doctor_args) => run_doctor(args, &doctor_args),
    }
}

fn run_doctor(global: &GlobalArgs, args: &SiblingsDoctorArgs) -> CliResult<CommandOutcome> {
    let cfg = load_config(global)?;
    let series = cfg.series_by_name(&args.series)?;
    let target = resolve_target(series, args)?;
    let repo = GitRepo::open(target.repo_dir.clone())?;
    let target_tip = repo.resolve_rev(target.branch.as_str())?;
    let self_project = cfg.projects.iter().find(|p| p.is_self());
    let since = resolve_since(&repo, self_project, &target.branch, &target_tip, args)?;
    let (vocabulary, vocabulary_source) =
        load_vocabulary(self_project, args.vocabulary_file_path.as_deref())?;
    // a branch repeated on the command line is one window, not two
    let mut source_branches: Vec<GitRef> = Vec::with_capacity(args.source_branches.len());
    for branch in &args.source_branches {
        if !source_branches.contains(branch) {
            source_branches.push(branch.clone());
        }
    }
    let source_tips = resolve_source_tips(&repo, &source_branches, &target.repo_dir)?;

    let computed = compute_with_cache(
        global,
        &cfg,
        args,
        &source_branches,
        &repo,
        &target,
        &target_tip,
        &since,
        &vocabulary,
        &source_tips,
    )?;

    let now = OffsetDateTime::now_utc();
    let mut candidates = computed.candidates;
    for candidate in &mut candidates {
        candidate.age_days = age_days(candidate.committed_at, now);
    }
    candidates.truncate(args.top);
    if !args.explain {
        candidates = candidates
            .into_iter()
            .map(SiblingCandidate::without_components)
            .collect();
    }
    let report = SiblingDoctorReport {
        series: args.series.clone(),
        target_branch: target.branch.clone(),
        source_branches,
        since: computed.since,
        vocabulary_source,
        suppressed_count: computed.suppressed_count,
        walked_count: computed.walked_count,
        candidates,
    };
    let exit = CommandOutcome::from_success(report.candidates.is_empty());
    emit_report(global, args, &repo, &report, exit)
}

/// The target branch and its repo directory.
struct ResolvedTarget {
    branch: GitRef,
    repo_dir: PathBuf,
}

/// `--series` names a pin set, not a branch; the series' self-pin is
/// the bridge. `--target-branch` overrides the ref; the self-pin's
/// `repo_dir_path` always wins over `--repo-dir-path` for locating
/// the repo, matching the `check` verbs.
fn resolve_target(series: &Series, args: &SiblingsDoctorArgs) -> CliResult<ResolvedTarget> {
    let self_pin = series.pins.iter().find(|p| p.is_self());
    let branch = match (&args.target_branch, self_pin) {
        (Some(explicit), _) => explicit.clone(),
        (None, Some(PinSpec::SelfRef { git_ref, .. })) => git_ref.clone(),
        (None, _) => {
            return Err(CliError::SeriesHasNoSelfPin {
                series: args.series.clone(),
            });
        }
    };
    let repo_dir = match self_pin {
        Some(spec) => effective_self_repo(spec, Some(&args.repo_dir_path))?.to_path_buf(),
        None => args.repo_dir_path.clone(),
    };
    Ok(ResolvedTarget { branch, repo_dir })
}

/// Resolve `--since`, or derive the default: the newest release tag
/// (by version order) whose commit is reachable from the target tip.
/// Reachability matters because the globally newest tag belongs to a
/// newer series.
fn resolve_since(
    repo: &GitRepo,
    self_project: Option<&Project>,
    target_branch: &GitRef,
    target_tip: &CommitSha,
    args: &SiblingsDoctorArgs,
) -> CliResult<SinceDerivation> {
    if let Some(raw) = args.since.as_deref() {
        if let Ok(tag) = TagName::new(raw)
            && let Ok(sha) = repo.resolve_tag(&tag)
        {
            return Ok(SinceDerivation::ExplicitTag { tag, sha });
        }
        let prefix = CommitShaPrefix::new(raw).map_err(|_| {
            CliError::InvalidInput(format!(
                "--since {raw:?} is neither a tag in this repository nor a commit SHA prefix"
            ))
        })?;
        let sha = expand_prefix_enriched(repo, &prefix, repo.path())?;
        return Ok(SinceDerivation::ExplicitSha { sha });
    }

    let listing = repo.list_tag_refs()?;
    let mut by_sha: Vec<(TagName, CommitSha)> = Vec::new();
    for tag in listing.tags {
        if let Some(project) = self_project {
            let pattern_ok = project
                .tag_pattern
                .as_ref()
                .is_none_or(|glob| glob.matches(&tag));
            if !pattern_ok || project.is_prerelease_tag(&tag) {
                continue;
            }
        }
        // unresolvable tags (e.g. pointing at blobs) just drop out
        if let Ok(sha) = repo.resolve_tag(&tag) {
            by_sha.push((tag, sha));
        }
    }
    let candidates: BTreeSet<CommitSha> = by_sha.iter().map(|(_, sha)| sha.clone()).collect();
    let reachable = repo.ancestors_among(target_tip, &candidates)?;
    // list_tag_refs sorts newest-first, so the first reachable hit is the last release
    let newest_reachable = by_sha.into_iter().find(|(_, sha)| reachable.contains(sha));
    match newest_reachable {
        Some((tag, sha)) => Ok(SinceDerivation::LastReleaseTag { tag, sha }),
        None => Err(CliError::NoReachableSinceTag {
            target_branch: target_branch.to_string(),
            pattern: self_project
                .and_then(|p| p.tag_pattern.as_ref())
                .map(|g| g.to_string()),
            shallow: repo.is_shallow(),
        }),
    }
}

fn load_vocabulary(
    self_project: Option<&Project>,
    file: Option<&Path>,
) -> CliResult<(Vocabulary, VocabularySource)> {
    let (terms, source) = match file {
        Some(path) => (read_vocabulary_file(path)?, VocabularySource::File),
        None => (
            self_project
                .map(|p| p.family.defaults().sibling_drift_vocabulary)
                .unwrap_or_default(),
            VocabularySource::FamilyDefault,
        ),
    };
    let vocabulary = Vocabulary::compile(&terms)
        .map_err(|e| CliError::InvalidInput(format!("vocabulary term failed to compile: {e}")))?;
    if vocabulary.is_empty() {
        tracing::warn!(
            "the effective sibling-drift vocabulary is empty; every commit fails the subject \
             prefilter and zero candidates will surface"
        );
        return Ok((vocabulary, VocabularySource::Empty));
    }
    Ok((vocabulary, source))
}

fn read_vocabulary_file(path: &Path) -> CliResult<Vec<String>> {
    let text = fs::read_to_string(path).map_err(|e| {
        CliError::InvalidInput(format!(
            "cannot read vocabulary file {}: {e}",
            path.display()
        ))
    })?;
    Ok(text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

fn resolve_source_tips(
    repo: &GitRepo,
    branches: &[GitRef],
    repo_dir: &Path,
) -> CliResult<Vec<(GitRef, CommitSha)>> {
    let mut out = Vec::with_capacity(branches.len());
    for branch in branches {
        let tip = repo.resolve_rev(branch.as_str()).map_err(|_| {
            CliError::InvalidInput(format!(
                "source branch {branch} does not resolve in {}; for a clone dedicated to \
                 another branch, try a remote-tracking ref like origin/{branch}",
                repo_dir.display()
            ))
        })?;
        out.push((branch.clone(), tip));
    }
    Ok(out)
}

/// The cacheable computation result: everything derived from repo
/// state, with `age_days` recomputed by the caller on every run so a
/// cached row never reports a stale age.
#[derive(Debug, Serialize, Deserialize)]
struct DoctorComputation {
    since: SinceDerivation,
    walked_count: u32,
    suppressed_count: u32,
    candidates: Vec<SiblingCandidate<Scored>>,
}

/// What identifies a cached run.
#[derive(Debug, Serialize)]
struct DoctorCacheKey<'a> {
    series: &'a str,
    target_branch: &'a str,
    source_branches: Vec<&'a str>,
    since_input: Option<&'a str>,
}

/// What makes a cached run servable. Any drift here is a miss.
#[derive(Debug, Serialize)]
struct DoctorCacheFreshness<'a> {
    source_tips: Vec<(&'a str, &'a str)>,
    target_tip: &'a str,
    since_sha: &'a str,
    vocabulary_hash: String,
    crate_version: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn compute_with_cache(
    global: &GlobalArgs,
    cfg: &Config,
    args: &SiblingsDoctorArgs,
    source_branches: &[GitRef],
    repo: &GitRepo,
    target: &ResolvedTarget,
    target_tip: &CommitSha,
    since: &SinceDerivation,
    vocabulary: &Vocabulary,
    source_tips: &[(GitRef, CommitSha)],
) -> CliResult<DoctorComputation> {
    let mode = CacheMode::resolve(args.no_cache);
    let key = DoctorCacheKey {
        series: args.series.as_str(),
        target_branch: target.branch.as_str(),
        source_branches: source_branches.iter().map(GitRef::as_str).collect(),
        since_input: args.since.as_deref(),
    };
    let freshness = DoctorCacheFreshness {
        source_tips: source_tips
            .iter()
            .map(|(branch, tip)| (branch.as_str(), tip.as_str()))
            .collect(),
        target_tip: target_tip.as_str(),
        since_sha: since.sha().as_str(),
        vocabulary_hash: content_hash(&vocabulary.terms())
            .map_err(|e| CliError::Other(e.to_string()))?,
        crate_version: env!("CARGO_PKG_VERSION"),
    };
    // the verdict cache's [cache] ttl_days governs this cache too
    let cache = CacheDir::new(snapshot_dir(global, cfg).join(CACHE_DIR_NAME))
        .with_max_age(ttl_from_days(cfg.cache.ttl_days));
    if mode.is_enabled()
        && let Ok(Some(hit)) = cache.lookup::<_, _, DoctorComputation>(&key, &freshness)
    {
        tracing::debug!("siblings doctor cache hit");
        return Ok(hit);
    }
    let computed = compute_candidates(repo, target, target_tip, since, vocabulary, source_tips)?;
    if mode.is_enabled()
        && let Err(e) = cache.store(&key, &freshness, &computed)
    {
        tracing::warn!("failed to write the siblings doctor cache: {e}");
    }
    Ok(computed)
}

fn compute_candidates(
    repo: &GitRepo,
    target: &ResolvedTarget,
    target_tip: &CommitSha,
    since: &SinceDerivation,
    vocabulary: &Vocabulary,
    source_tips: &[(GitRef, CommitSha)],
) -> CliResult<DoctorComputation> {
    let mut walked_count: u32 = 0;
    let mut suppressed_count: u32 = 0;
    let mut candidates: Vec<SiblingCandidate<Scored>> = Vec::new();

    if vocabulary.is_empty() {
        // count the window so the report stays meaningful, but skip the target index, suppression walk, and diffs
        for (_, tip) in source_tips {
            let window = first_parent_walk_since(repo, tip, since.sha())?;
            walked_count = walked_count.saturating_add(window.len() as u32);
        }
        return Ok(DoctorComputation {
            since: since.clone(),
            walked_count,
            suppressed_count,
            candidates,
        });
    }

    let target_index = build_target_tree_index(repo, &target.branch)?;
    let suppression = build_suppression_set(repo, target_tip, since.sha())?;
    let weights = ScoreWeights::default();
    let thresholds = ActionThresholds::default();
    let mut seen_shas: BTreeSet<CommitSha> = BTreeSet::new();
    let mut seen_patch_ids: BTreeSet<PatchId> = BTreeSet::new();

    for (_, tip) in source_tips {
        let window = first_parent_walk_since(repo, tip, since.sha())?;
        walked_count = walked_count.saturating_add(window.len() as u32);
        for commit in window {
            if commit.parents.is_empty() || !seen_shas.insert(commit.sha.clone()) {
                continue;
            }
            let subject = effective_subject(&commit);
            if vocabulary.distinct_matches(&subject) == 0 {
                continue;
            }
            let input = ResolvedPatchInput::from_parents(
                repo,
                &commit.sha,
                &commit.parents,
                MergePolicy::FirstParentDiff,
                PrCommitPolicy::Skip,
            )?;
            let (touched_paths, lines_added, lines_removed) = patch_facts(&input.bytes)?;
            let on_target = touched_paths
                .iter()
                .any(|p| target_index.contains_path(Path::new(p.as_str())));
            if !on_target {
                continue;
            }
            let candidate_patch_id = patch_id(repo, &input.diff_base, &commit.sha)?;
            if suppression.shas.contains(&commit.sha)
                || candidate_patch_id
                    .as_ref()
                    .is_some_and(|id| suppression.patch_ids.contains(id))
            {
                suppressed_count += 1;
                continue;
            }
            // the same fix cherry-picked onto a second source branch is one candidate, not two
            if let Some(id) = candidate_patch_id
                && !seen_patch_ids.insert(id)
            {
                continue;
            }
            let unscored = SiblingCandidate {
                sha: commit.sha,
                subject,
                committed_at: commit.committed_at,
                age_days: 0,
                touched_paths,
                parent_count: input.source.parent_count(),
                state: Unscored {
                    lines_added,
                    lines_removed,
                },
            };
            candidates.push(unscored.score(vocabulary, &weights, &thresholds));
        }
    }

    candidates.sort_by(|a, b| {
        b.confidence()
            .cmp(&a.confidence())
            .then_with(|| a.committed_at.cmp(&b.committed_at))
            .then_with(|| a.sha.cmp(&b.sha))
    });
    Ok(DoctorComputation {
        since: since.clone(),
        walked_count,
        suppressed_count,
        candidates,
    })
}

struct SuppressionSet {
    shas: BTreeSet<CommitSha>,
    patch_ids: BTreeSet<PatchId>,
}

/// One walk of the target branch since the since-point. `-x` trailers
/// catch disciplined picks by source SHA; patch-ids catch hand-landed
/// picks by content.
fn build_suppression_set(
    repo: &GitRepo,
    target_tip: &CommitSha,
    since_sha: &CommitSha,
) -> CliResult<SuppressionSet> {
    let mut shas: BTreeSet<CommitSha> = BTreeSet::new();
    let mut patch_ids: BTreeSet<PatchId> = BTreeSet::new();
    let window = first_parent_walk_since(repo, target_tip, since_sha)?;
    for commit in window {
        shas.extend(cherry_pick_trailers(&commit.message));
        if let Some(base) = commit.parents.first()
            && let Some(id) = patch_id(repo, base, &commit.sha)?
        {
            patch_ids.insert(id);
        }
    }
    Ok(SuppressionSet { shas, patch_ids })
}

/// Touched paths plus added and removed line counts of a unified diff.
fn patch_facts(bytes: &[u8]) -> CliResult<(Vec<RelativePath>, u32, u32)> {
    if bytes.is_empty() {
        return Ok((Vec::new(), 0, 0));
    }
    let patch = Patch::parse(bytes)?;
    let mut paths: Vec<RelativePath> = Vec::new();
    let mut added: u32 = 0;
    let mut removed: u32 = 0;
    for file in &patch.files {
        let path = file.primary_path();
        if let Some(p) = path
            && let Some(s) = p.to_str()
            && let Ok(rel) = RelativePath::new(s.to_owned())
            && !paths.contains(&rel)
        {
            paths.push(rel);
        }
        for hunk in &file.hunks {
            for line in &hunk.lines {
                match line {
                    HunkLine::Added(_) => added = added.saturating_add(1),
                    HunkLine::Removed(_) => removed = removed.saturating_add(1),
                    HunkLine::Context(_) => {}
                }
            }
        }
    }
    Ok((paths, added, removed))
}

/// The line worth matching and showing. A GitHub PR merge's subject
/// is boilerplate (`Merge pull request #N from org/branch`); the PR
/// title sits on the first body line, so that line carries the useful
/// text on a PR-merge-dominated branch like RabbitMQ's `main`.
fn effective_subject(commit: &WalkedCommit) -> String {
    if commit.parents.len() >= 2
        && commit.subject.starts_with("Merge pull request ")
        && let Some(title) = first_body_line(&commit.message)
    {
        return title;
    }
    commit.subject.clone()
}

fn first_body_line(message: &str) -> Option<String> {
    let mut lines = message.lines();
    for line in lines.by_ref() {
        if line.trim().is_empty() {
            break;
        }
    }
    lines
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

fn age_days(committed_at: OffsetDateTime, now: OffsetDateTime) -> u32 {
    let days = (now - committed_at).whole_days();
    u32::try_from(days).unwrap_or(0)
}

fn emit_report(
    global: &GlobalArgs,
    args: &SiblingsDoctorArgs,
    repo: &GitRepo,
    report: &SiblingDoctorReport,
    exit: CommandOutcome,
) -> CliResult<CommandOutcome> {
    if let Some(fmt) = SummaryFormatter::from_cli(global.formatter) {
        let mut stdout = io::stdout().lock();
        match fmt {
            SummaryFormatter::Jsonl => emit_jsonl(&mut stdout, &report.candidates)?,
            SummaryFormatter::Text => {
                for candidate in &report.candidates {
                    writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        candidate.sha.abbreviated(),
                        candidate.action().as_str(),
                        candidate.confidence(),
                        candidate.subject.replace('\t', " "),
                    )?;
                }
            }
        }
        return Ok(exit);
    }
    // the column renders only in the table forms; skip the per-branch ancestry walks under the JSON formatter
    let wants_table = matches!(global.formatter, Formatter::Text | Formatter::Markdown);
    let branch_columns = if args.with_branches && wants_table {
        Some(branch_containment(repo, &report.candidates)?)
    } else {
        None
    };
    let style = global.table_style;
    let ctx = OutputContext::new(global.formatter, "siblings doctor");
    render_with_exit(&ctx, report, exit, |w| {
        render_doctor_text(w, report, branch_columns.as_ref(), style)
    })
}

/// Local branches containing each surfaced candidate: one ancestry
/// walk per branch, not per (candidate, branch) pair.
fn branch_containment(
    repo: &GitRepo,
    candidates: &[SiblingCandidate<Scored>],
) -> CliResult<BTreeMap<CommitSha, Vec<String>>> {
    let shas: BTreeSet<CommitSha> = candidates.iter().map(|c| c.sha.clone()).collect();
    let mut out: BTreeMap<CommitSha, Vec<String>> = BTreeMap::new();
    for branch in repo.local_branch_tips()? {
        let (name, tip) = branch;
        let contained = repo.ancestors_among(&tip, &shas)?;
        for sha in contained {
            out.entry(sha).or_default().push(name.clone());
        }
    }
    Ok(out)
}

#[derive(Tabled)]
struct CandidateRow {
    #[tabled(rename = "SHA")]
    sha: String,
    #[tabled(rename = "SUBJECT")]
    subject: String,
    #[tabled(rename = "CONF")]
    confidence: String,
    #[tabled(rename = "ACTION")]
    action: &'static str,
    #[tabled(rename = "AGE")]
    age: String,
}

#[derive(Tabled)]
struct CandidateRowWithBranches {
    #[tabled(rename = "SHA")]
    sha: String,
    #[tabled(rename = "SUBJECT")]
    subject: String,
    #[tabled(rename = "CONF")]
    confidence: String,
    #[tabled(rename = "ACTION")]
    action: &'static str,
    #[tabled(rename = "AGE")]
    age: String,
    #[tabled(rename = "BRANCHES")]
    branches: String,
}

pub fn render_doctor_text(
    w: &mut dyn Write,
    report: &SiblingDoctorReport,
    branch_columns: Option<&BTreeMap<CommitSha, Vec<String>>>,
    style: bel7_cli::TableStyle,
) -> CliResult<()> {
    if report.candidates.is_empty() {
        match report.vocabulary_source {
            // An empty vocabulary analysed nothing: not the same as no drift.
            VocabularySource::Empty => writeln!(
                w,
                "no vocabulary in effect: this run matched nothing and does not bound sibling-drift risk"
            )?,
            VocabularySource::FamilyDefault | VocabularySource::File => {
                writeln!(w, "no sibling-drift candidates found")?;
            }
        }
    } else if let Some(by_sha) = branch_columns {
        let rows: Vec<CandidateRowWithBranches> = report
            .candidates
            .iter()
            .map(|c| CandidateRowWithBranches {
                sha: c.sha.abbreviated().to_owned(),
                subject: c.subject.clone(),
                confidence: c.confidence().to_string(),
                action: c.action().as_str(),
                age: format_age(c.age_days),
                branches: by_sha
                    .get(&c.sha)
                    .map_or_else(|| "-".to_owned(), |b| b.join(", ")),
            })
            .collect();
        writeln!(w, "{}", styled_table(rows, style))?;
    } else {
        let rows: Vec<CandidateRow> = report
            .candidates
            .iter()
            .map(|c| CandidateRow {
                sha: c.sha.abbreviated().to_owned(),
                subject: c.subject.clone(),
                confidence: c.confidence().to_string(),
                action: c.action().as_str(),
                age: format_age(c.age_days),
            })
            .collect();
        writeln!(w, "{}", styled_table(rows, style))?;
    }
    let branches = report.source_branches.len();
    writeln!(
        w,
        "suppressed {} already-cascaded commit{}; walked {} across {} source branch{}",
        report.suppressed_count,
        if report.suppressed_count == 1 {
            ""
        } else {
            "s"
        },
        report.walked_count,
        branches,
        if branches == 1 { "" } else { "es" },
    )?;
    Ok(())
}

/// Compact age for the table: days under ~2 months, months after.
fn format_age(days: u32) -> String {
    if days < 60 {
        format!("{days}d")
    } else {
        format!("{}mo", days / 30)
    }
}
