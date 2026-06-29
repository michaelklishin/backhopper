// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Bisect a commit's verdict across every stored tag of a project.

use bel7_cli::PARTIAL_SUCCESS_I32;
use serde::Serialize;

use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::config::{Config, Project};
use backhopper_core::model::names::{CommitSha, ModuleName, ProjectName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::Snapshot;
use backhopper_core::model::snapshot::state::Canonical;
use backhopper_core::model::verdict::{SeriesEvaluation, Verdict};
use backhopper_core::versions::version_cmp;

use crate::cli::{BisectCmd, GlobalArgs};
use backhopper_git::{
    GitRepo, MergePolicy, PrCommitPolicy, ResolvedPatchInput, normalized_patch_hash,
};

use crate::commands::context::{load_config, open_store_read};
use crate::commands::sha_prefix::{enrich_with_repo_path, expand_prefix_with};
use crate::commands::verdict_cache::{CacheLookupOutcome, CacheSession, MacroEnv, SessionLookup};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

#[derive(Debug, Serialize)]
struct BisectRow {
    tag: String,
    verdict: String,
}

#[derive(Debug, Serialize)]
struct BisectPayload {
    project: String,
    commit: CommitSha,
    rows: Vec<BisectRow>,
    last_compatible_tag: Option<String>,
    first_incompatible_tag: Option<String>,
    first_requires_adaptation_tag: Option<String>,
}

pub fn handle(args: &GlobalArgs, cmd: BisectCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        BisectCmd::Commit {
            project,
            repo_dir_path,
            no_cache,
            commit,
        } => {
            let repo = GitRepo::open(repo_dir_path.clone())?;
            let commit = expand_prefix_with(&repo, &commit)
                .map_err(|e| enrich_with_repo_path(e, &repo_dir_path))?;
            run_commit(args, &cfg, project, &repo, &commit, no_cache)
        }
    }
}

fn run_commit(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    repo: &GitRepo,
    commit: &CommitSha,
    no_cache: bool,
) -> CliResult<i32> {
    let p = cfg.project(&project)?;
    let store = open_store_read(args, cfg)?;
    let mut tags = store.list_tags(&project)?;
    if tags.is_empty() {
        return Err(CliError::InvalidInput(format!(
            "no snapshots on disk for project {project}"
        )));
    }
    tags.sort_by(|a, b| version_cmp(b.as_str(), a.as_str()));
    let input =
        ResolvedPatchInput::for_commit(repo, commit, MergePolicy::Refuse, PrCommitPolicy::Skip)?;
    let patch = Patch::parse(&input.bytes)?;
    let analyzed = patch.analyze();
    let session = CacheSession::open(args, cfg, no_cache, false);
    let patch_blake3 = normalized_patch_hash(&input.bytes).unwrap_or_else(|| "empty".to_owned());
    // one evaluation per tag, so a re-bisect after the store grows
    // reuses every overlapping (commit, tag) pair from the cache
    let mut verdicts: Vec<Verdict> = Vec::with_capacity(tags.len());
    for tag in &tags {
        let evaluate = || -> CliResult<SeriesEvaluation> {
            let snap = store.read(&project, tag)?;
            let scope = build_pin_scope(p, &snap);
            let pin = Pin::new(project.clone(), tag.clone());
            let ctx = EvaluationContext::new(pin, snap, scope);
            Ok(analyzed.clone().evaluate_series(&[ctx]))
        };
        let key = session.bisect_key(&store, commit, &project, tag);
        let evaluation = match session.lookup(key) {
            SessionLookup::Hit(cached) => *cached.evaluation,
            SessionLookup::Bypassed => evaluate()?,
            SessionLookup::Miss(miss) => {
                match session.consult_content(*miss, patch_blake3.clone(), &MacroEnv::Unused) {
                    CacheLookupOutcome::Hit(cached) => *cached.evaluation,
                    CacheLookupOutcome::Miss(slot) => {
                        let evaluation = evaluate()?;
                        slot.store(&evaluation);
                        evaluation
                    }
                }
            }
        };
        let verdict = evaluation
            .verdict
            .results
            .into_iter()
            .next()
            .map(|pv| pv.verdict)
            .ok_or_else(|| CliError::Other("bisect evaluation produced no verdict row".into()))?;
        verdicts.push(verdict);
    }
    session.report();
    let mut rows: Vec<BisectRow> = Vec::with_capacity(tags.len());
    let mut last_compatible_tag: Option<String> = None;
    let mut first_incompatible_tag: Option<String> = None;
    let mut first_requires_adaptation_tag: Option<String> = None;
    for (tag, verdict) in tags.iter().zip(verdicts.iter()) {
        let verdict_str = verdict_label(verdict);
        match verdict {
            Verdict::Compatible => {
                last_compatible_tag = Some(tag.to_string());
            }
            Verdict::RequiresAdaptation { .. } => {
                if first_requires_adaptation_tag.is_none() {
                    first_requires_adaptation_tag = Some(tag.to_string());
                }
            }
            Verdict::Incompatible { .. } => {
                if first_incompatible_tag.is_none() {
                    first_incompatible_tag = Some(tag.to_string());
                }
            }
            // Bisect doesn't run inapplicable promotion, so Inapplicable shouldn't appear.
            Verdict::Inapplicable { .. } => {}
        }
        rows.push(BisectRow {
            tag: tag.to_string(),
            verdict: verdict_str.into(),
        });
    }
    let any_blocking = first_incompatible_tag.is_some();
    let payload = BisectPayload {
        project: project.to_string(),
        commit: commit.clone(),
        rows,
        last_compatible_tag,
        first_incompatible_tag,
        first_requires_adaptation_tag,
    };
    let exit = if any_blocking { PARTIAL_SUCCESS_I32 } else { 0 };
    let ctx = OutputContext::new(args.formatter, "bisect commit");
    render_with_exit(&ctx, &payload, exit, |w| {
        for r in &payload.rows {
            writeln!(w, "{}\t{}", r.tag, r.verdict)?;
        }
        writeln!(w)?;
        match &payload.last_compatible_tag {
            Some(t) => writeln!(w, "last compatible: {t}")?,
            None => writeln!(w, "last compatible: (none)")?,
        }
        if let Some(t) = &payload.first_incompatible_tag {
            writeln!(w, "first incompatible: {t}")?;
        }
        if let Some(t) = &payload.first_requires_adaptation_tag {
            writeln!(w, "first requires_adaptation: {t}")?;
        }
        Ok(())
    })
}

fn build_pin_scope(p: &Project, snap: &Snapshot<Canonical>) -> PinScope {
    let extra: Vec<ModuleName> = p
        .public_modules
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    PinScope::from_snapshot(p.name.clone(), snap, extra)
}

fn verdict_label(v: &Verdict) -> &'static str {
    match v {
        Verdict::Compatible => "compatible",
        Verdict::RequiresAdaptation { .. } => "requires_adaptation",
        Verdict::Incompatible { .. } => "incompatible",
        Verdict::Inapplicable { .. } => "inapplicable",
    }
}
