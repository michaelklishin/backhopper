// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use backhopper_core::Error as CoreError;
use backhopper_core::config::{Config, Host, Project};
use backhopper_core::errors::StoreError;
use backhopper_core::jigsaw::plugin_set::PluginSet;
use backhopper_core::jigsaw::{
    JigsawCheckPayload, JigsawPlan, JigsawPlanPayload, ListedTags, PlanError, ResolvedRelease,
    SeriesRequest, StoreStatus, TagSourceKind, evaluate, plan_series, plan_series_from_store,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::pin::Pin;
use backhopper_core::model::release_version::{ErlangMajor, HostTag};
use backhopper_core::model::wire_envelope::WireEnvelope;
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};
use backhopper_git::GitRepo;
use backhopper_jigsaw::collect;

use crate::cli::GlobalArgs;
use crate::cli::jigsaw::{CheckArgs, JigsawCmd, PlanArgs};
use crate::commands::auto_generate::{
    ExpectedExtractions, expected_extraction, generate_host_pin, generate_one_pin,
};
use crate::commands::context::{load_config, open_project_repo, open_store_mut, open_store_read};
use crate::commands::host_facts::HostFacts;
use crate::commands::jigsaw_render::{
    write_check_markdown, write_check_text, write_plan_markdown, write_plan_text,
};
use crate::commands::parallel::{Jobs, run_in_parallel};
use crate::commands::plugin_tree;
use crate::errors::{CliError, CliResult};
use crate::outcome::CommandOutcome;
use crate::output::{OutputContext, envelope_json, render_with_alts};

const PLAN_COMMAND: &str = "jigsaw plan";
const CHECK_COMMAND: &str = "jigsaw check";

pub fn handle(args: &GlobalArgs, cmd: JigsawCmd) -> CliResult<CommandOutcome> {
    match cmd {
        JigsawCmd::Plan(plan_args) => plan(args, plan_args),
        JigsawCmd::Check(check_args) => check(args, check_args),
    }
}

fn plan(args: &GlobalArgs, plan_args: PlanArgs) -> CliResult<CommandOutcome> {
    let cfg = load_config(args)?;
    let host = cfg.host(&plan_args.host)?;
    let host_project = cfg.project(&host.project)?;
    let otp_project = cfg.project(&host.otp_project)?;
    let host_repo = match &plan_args.repo_dir_path {
        Some(path) => GitRepo::open(path.clone())?,
        None => open_project_repo(host_project)?,
    };
    let otp_repo = open_project_repo(otp_project)?;
    let store = open_store_if_present(args, &cfg)?;
    let extractors = ExpectedExtractions::for_config(&cfg)?;
    let facts = HostFacts::for_host(host);
    let request = SeriesRequest {
        series: plan_args.series,
        up_to: plan_args.up_to,
        majors: requested_majors(plan_args.erlang_majors, host),
    };
    let listed = ListedTags {
        host: host_repo.list_tags()?,
        otp: otp_repo.list_tags()?,
    };
    let drafted = plan_series(
        &cfg,
        host,
        &request,
        &listed,
        TagSourceKind::Repository,
        expected_extraction(host_project)?.extractor,
        |tag| resolve_release(host_project, &host_repo, store.as_ref(), &facts, host, tag),
    )?;
    for skipped in &drafted.skipped_tags {
        tracing::warn!(tag = %skipped, "tag matches the host's pattern but is not a release version");
    }
    let plan = drafted.plan;
    let status = match &store {
        Some(store) => plan.store_status(store, |project| extractors.get(project)),
        None => plan.nothing_stored(),
    };
    let payload = plan.to_wire(&status);
    let outcome = CommandOutcome::from_success(matches!(status, StoreStatus::Stocked));
    let ctx = OutputContext::from_args(args, PLAN_COMMAND);
    if let Some(path) = &plan_args.plan_file_path {
        fs::write(path, envelope_json(&ctx, &payload, outcome.exit_code())?)?;
    }
    render_with_alts(
        &ctx,
        &payload,
        outcome,
        |w| write_plan_text(w, &payload, args.table_style),
        |w| write_plan_markdown(w, &payload),
    )
}

fn requested_majors(requested: Vec<ErlangMajor>, host: &Host) -> NonEmpty<ErlangMajor> {
    let mut majors = NonEmpty::try_from(requested).unwrap_or_else(|_| host.erlang_majors.clone());
    majors.sort_dedup();
    majors
}

fn open_store_if_present(
    args: &GlobalArgs,
    cfg: &Config,
) -> CliResult<Option<SnapshotStore<ReadOnly>>> {
    match open_store_read(args, cfg) {
        Ok(store) => Ok(Some(store)),
        Err(CliError::Core(CoreError::Store(StoreError::RootMissing(_)))) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A release's commit comes from the repository. Its pins come from a
/// stored host snapshot only when that snapshot is for the same commit and
/// was generated with the same host facts; otherwise they are read at the
/// commit.
fn resolve_release(
    host_project: &Project,
    repo: &GitRepo,
    store: Option<&SnapshotStore<ReadOnly>>,
    facts: &HostFacts,
    host: &Host,
    tag: &HostTag,
) -> Result<ResolvedRelease, PlanError> {
    let unreadable = |detail: String| PlanError::ReleaseUnreadable {
        tag: tag.tag().clone(),
        detail,
    };
    let commit = repo
        .resolve_tag(tag.tag())
        .map_err(|e| unreadable(e.to_string()))?;
    let stored = store
        .and_then(|store| store.read_header(&host.project, tag.tag()).ok())
        .filter(|header| {
            header.commit == commit
                && header
                    .host_facts
                    .as_ref()
                    .is_some_and(|stored| stored.digest == facts.digest)
        });
    let pins = match stored {
        Some(header) => header.dep_pins,
        None => facts
            .read_dep_pins(host_project, repo, &commit, tag.tag())
            .map_err(|e| unreadable(e.to_string()))?,
    };
    Ok(ResolvedRelease { commit, pins })
}

pub fn read_plan_file(path: &Path, cfg: &Config) -> CliResult<JigsawPlan> {
    let text = fs::read_to_string(path)?;
    let envelope: WireEnvelope<JigsawPlanPayload> = serde_json::from_str(&text)
        .map_err(|e| CliError::InvalidInput(format!("{}: {e}", path.display())))?;
    if envelope.command.as_deref() != Some(PLAN_COMMAND) {
        return Err(CliError::InvalidInput(format!(
            "{} is not a `{PLAN_COMMAND}` envelope",
            path.display()
        )));
    }
    Ok(JigsawPlan::from_wire(&envelope.data, cfg)?)
}

#[derive(Debug, Serialize)]
struct GeneratedPin {
    project: String,
    tag: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct FailedPin {
    project: String,
    tag: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct GenerateFromPlanPayload {
    cache_key: String,
    dry_run: bool,
    generated: Vec<GeneratedPin>,
    failed: Vec<FailedPin>,
}

/// Builds what the plan lists as unusable in the store, and nothing else:
/// the plan is not recomputed, so a tag pushed after it was drafted waits
/// for the next `jigsaw plan`.
pub fn generate_from_plan(
    args: &GlobalArgs,
    cfg: &Config,
    plan_path: &Path,
    dry_run: bool,
    jobs: Jobs,
) -> CliResult<CommandOutcome> {
    let plan = read_plan_file(plan_path, cfg)?;
    let host = cfg.host(plan.host())?;
    let facts = HostFacts::for_host(host);
    if facts.digest != *plan.host_facts() {
        return Err(CliError::InvalidInput(format!(
            "the host pins or release roots in the configuration differ from those plan {} was drafted with: run `jigsaw plan` again",
            plan.cache_key()
        )));
    }
    let store = open_store_mut(args, cfg)?;
    let readable = open_store_read(args, cfg)?;
    let extractors = ExpectedExtractions::for_config(cfg)?;
    let gaps = match plan.store_status(&readable, |project| extractors.get(project)) {
        StoreStatus::Stocked => Vec::new(),
        StoreStatus::Unstocked(unstocked) => unstocked.gaps(),
    };
    let mut generated = Vec::new();
    let mut failed = Vec::new();
    let results = run_in_parallel(
        jobs,
        &gaps,
        || (),
        |(), (pin, _)| {
            if dry_run {
                Ok(())
            } else {
                generate_plan_pin(cfg, &store, &plan, &facts, pin).map_err(|e| e.to_string())
            }
        },
        |(pin, _), result| match result {
            Ok(()) => tracing::info!(project = %pin.project, tag = %pin.tag, "snapshot written"),
            Err(error) => {
                tracing::warn!(project = %pin.project, tag = %pin.tag, %error, "snapshot not written");
            }
        },
    );
    for ((pin, gap), result) in gaps.into_iter().zip(results) {
        match result {
            Ok(()) => generated.push(GeneratedPin {
                project: pin.project.to_string(),
                tag: pin.tag.to_string(),
                reason: gap.to_string(),
            }),
            Err(error) => failed.push(FailedPin {
                project: pin.project.to_string(),
                tag: pin.tag.to_string(),
                error,
            }),
        }
    }
    let payload = GenerateFromPlanPayload {
        cache_key: plan.cache_key().to_string(),
        dry_run,
        generated,
        failed,
    };
    let outcome = CommandOutcome::from_success(payload.failed.is_empty());
    let ctx = OutputContext::from_args(args, "snapshots generate");
    let render_text = |w: &mut dyn Write| write_generate_text(w, &payload);
    render_with_alts(&ctx, &payload, outcome, render_text, render_text)
}

fn generate_plan_pin(
    cfg: &Config,
    store: &SnapshotStore<Mutable>,
    plan: &JigsawPlan,
    facts: &HostFacts,
    pin: &Pin,
) -> CliResult<()> {
    let host_release = (&pin.project == plan.host_project())
        .then(|| {
            plan.releases()
                .iter()
                .find(|release| release.tag.tag() == &pin.tag)
        })
        .flatten();
    match host_release {
        Some(release) => generate_host_pin(cfg, store, facts, pin, &release.commit),
        None => generate_one_pin(cfg, store, pin),
    }
}

fn write_generate_text(w: &mut dyn Write, payload: &GenerateFromPlanPayload) -> CliResult<()> {
    let verb = if payload.dry_run {
        "would generate"
    } else {
        "generated"
    };
    writeln!(
        w,
        "plan {}: {verb} {}, failed {}",
        payload.cache_key,
        payload.generated.len(),
        payload.failed.len()
    )?;
    for pin in &payload.generated {
        writeln!(w, "  {} {} ({})", pin.project, pin.tag, pin.reason)?;
    }
    for pin in &payload.failed {
        writeln!(w, "  failed {} {}: {}", pin.project, pin.tag, pin.error)?;
    }
    Ok(())
}

/// `jigsaw check` reads the plan, the snapshot store and the plugin
/// directories, and never opens a git repository.
fn check(args: &GlobalArgs, check_args: CheckArgs) -> CliResult<CommandOutcome> {
    let cfg = load_config(args)?;
    let extractors = ExpectedExtractions::for_config(&cfg)?;
    let store = open_store_if_present(args, &cfg)?;
    let plan = check_plan(&cfg, &check_args, store.as_ref())?;
    let loaded = match &store {
        Some(store) => plan.load(store, |project| extractors.get(project)),
        None => Err(plan.all_missing()),
    }
    .map_err(CliError::UnstockedPlan)?;
    let sources = plugin_tree::walk(&check_args.plugin_dir_paths, loaded.build_app_names())?;
    let plugins = PluginSet::new(sources)?;
    let report = evaluate(&loaded, &plugins, collect)?;
    let payload = JigsawCheckPayload::from(&report);
    let outcome = CommandOutcome::from_success(payload.clean);
    let ctx = OutputContext::from_args(args, CHECK_COMMAND);
    render_with_alts(
        &ctx,
        &payload,
        outcome,
        |w| write_check_text(w, &payload, args.table_style),
        |w| write_check_markdown(w, &payload),
    )
}

fn check_plan(
    cfg: &Config,
    check_args: &CheckArgs,
    store: Option<&SnapshotStore<ReadOnly>>,
) -> CliResult<JigsawPlan> {
    if let Some(path) = &check_args.plan_file_path {
        return read_plan_file(path, cfg);
    }
    let (Some(host_name), Some(series)) = (&check_args.host, check_args.series) else {
        return Err(CliError::InvalidInput(
            "pass --plan-file-path, or --host with --series".to_owned(),
        ));
    };
    let Some(store) = store else {
        return Err(CliError::Core(CoreError::Store(StoreError::RootMissing(
            cfg.snapshot_dir(),
        ))));
    };
    let host = cfg.host(host_name)?;
    let request = SeriesRequest {
        series,
        up_to: check_args.up_to.clone(),
        majors: requested_majors(check_args.erlang_majors.clone(), host),
    };
    Ok(plan_series_from_store(
        cfg,
        host,
        &request,
        store,
        expected_extraction(cfg.project(&host.project)?)?.extractor,
    )?
    .plan)
}
