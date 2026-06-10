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
use backhopper_core::model::verdict::Verdict;
use backhopper_core::versions::version_cmp;

use crate::cli::{BisectCmd, GlobalArgs};
use backhopper_git::{GitRepo, MergePolicy, PrCommitPolicy, ResolvedPatchInput};

use crate::commands::context::{load_config, open_store_read};
use crate::commands::sha_prefix::{enrich_with_repo_path, expand_prefix_with};
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
            commit,
        } => {
            let repo = GitRepo::open(repo_dir_path.clone()).map_err(CliError::Git)?;
            let commit = expand_prefix_with(&repo, &commit)
                .map_err(|e| enrich_with_repo_path(e, &repo_dir_path))?;
            run_commit(args, &cfg, project, &repo, &commit)
        }
    }
}

fn run_commit(
    args: &GlobalArgs,
    cfg: &Config,
    project: ProjectName,
    repo: &GitRepo,
    commit: &CommitSha,
) -> CliResult<i32> {
    let p = cfg
        .project(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    let store = open_store_read(args, cfg)?;
    let mut tags = store
        .list_tags(&project)
        .map_err(|e| CliError::Core(e.into()))?;
    if tags.is_empty() {
        return Err(CliError::InvalidInput(format!(
            "no snapshots on disk for project {project}"
        )));
    }
    tags.sort_by(|a, b| version_cmp(b.as_str(), a.as_str()));
    let input =
        ResolvedPatchInput::for_commit(repo, commit, MergePolicy::Refuse, PrCommitPolicy::Skip)?;
    let patch = Patch::parse(&input.bytes).map_err(|e| CliError::Core(e.into()))?;
    let analyzed = patch.analyze();
    let mut contexts: Vec<EvaluationContext> = Vec::with_capacity(tags.len());
    for tag in &tags {
        let snap = store
            .read(&project, tag)
            .map_err(|e| CliError::Core(e.into()))?;
        let scope = build_pin_scope(p, &snap);
        let pin = Pin::new(project.clone(), tag.clone());
        contexts.push(EvaluationContext::new(pin, snap, scope));
    }
    let evaluation = analyzed.evaluate_series(&contexts);
    let mut rows: Vec<BisectRow> = Vec::with_capacity(tags.len());
    let mut last_compatible_tag: Option<String> = None;
    let mut first_incompatible_tag: Option<String> = None;
    let mut first_requires_adaptation_tag: Option<String> = None;
    for (tag, pin_verdict) in tags.iter().zip(evaluation.verdict.results.iter()) {
        let verdict_str = verdict_label(&pin_verdict.verdict);
        match &pin_verdict.verdict {
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
