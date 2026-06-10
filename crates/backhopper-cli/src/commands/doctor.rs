// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::io::Write;

use bel7_cli::{PARTIAL_SUCCESS_I32, TableStyle};
use serde::Serialize;
use tabled::{Table, Tabled};

use backhopper_cache::{WorkspaceCaches, stats};
use backhopper_core::config::{Config, Project, Series};
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
use backhopper_core::store::{ReadOnly, SnapshotStore};
use backhopper_core::versions::version_cmp;

use backhopper_git::GitRepo;

use crate::cli::{DoctorCmd, GlobalArgs};
use crate::commands::context::{load_config, open_store_read, snapshot_dir};
use crate::commands::snapshots::filter_tags_for_project;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

#[derive(Debug, Serialize)]
struct DoctorPayload {
    config_path: String,
    snapshot_dir: String,
    series: Vec<SeriesRow>,
    totals: Totals,
    unpinned_projects: Vec<String>,
    cache: CacheRow,
}

/// One informational row about the workspace caches: never affects
/// the exit code.
#[derive(Debug, Serialize)]
struct CacheRow {
    entries: u64,
    bytes: u64,
}

/// Past this the text renderer suggests `cache prune`.
const CACHE_SIZE_HINT_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct Totals {
    series: usize,
    pins: usize,
    covered: usize,
    missing: usize,
}

#[derive(Debug, Clone, Serialize)]
struct SeriesRow {
    name: String,
    pins: Vec<PinRow>,
}

#[derive(Debug, Clone, Serialize)]
struct PinRow {
    project: ProjectName,
    pin: PinDisplay,
    resolved_tag: Option<TagName>,
    snapshot_present: bool,
    upstream_tags_ahead: Option<usize>,
    note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PinDisplay {
    Literal { tag: String },
    Pattern { pattern: String, select: String },
    SelfRef { git_ref: String },
}

pub fn handle(args: &GlobalArgs, cmd: DoctorCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    let store = open_store_read(args, &cfg)?;
    let payload = build_payload(args, &cfg, &store, &cmd)?;
    let exit = doctor_exit_code(payload.totals.missing);
    let style = args.table_style;
    let ctx = OutputContext::new(args.formatter, "doctor");
    render_with_exit(&ctx, &payload, exit, |w| render_text(w, &payload, style))
}

// 0 when every pin covered or empty config; 3 when any pin missing
pub fn doctor_exit_code(missing: usize) -> i32 {
    if missing == 0 { 0 } else { PARTIAL_SUCCESS_I32 }
}

fn build_payload(
    args: &GlobalArgs,
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    cmd: &DoctorCmd,
) -> CliResult<DoctorPayload> {
    let snapshots = snapshot_dir(args, cfg);
    let mut totals = Totals {
        series: 0,
        pins: 0,
        covered: 0,
        missing: 0,
    };
    let mut series_rows: Vec<SeriesRow> = Vec::new();
    for series in &cfg.series {
        if let Some(filter) = &cmd.series
            && !series.name.as_str().contains(filter.as_str())
        {
            continue;
        }
        let row = build_series_row(cfg, store, series, cmd.check_remote)?;
        for pin in &row.pins {
            totals.pins += 1;
            if pin.snapshot_present {
                totals.covered += 1;
            } else {
                totals.missing += 1;
            }
        }
        series_rows.push(row);
        totals.series += 1;
    }
    let cache_stats = stats(&WorkspaceCaches::at(&snapshots));
    Ok(DoctorPayload {
        config_path: cfg.config_path.display().to_string(),
        snapshot_dir: snapshots.display().to_string(),
        series: series_rows,
        totals,
        unpinned_projects: unpinned_projects(cfg),
        cache: CacheRow {
            entries: cache_stats.by_input.entries
                + cache_stats.by_content.entries
                + cache_stats.siblings.entries,
            bytes: cache_stats.total_bytes,
        },
    })
}

fn build_series_row(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    series: &Series,
    check_remote: bool,
) -> CliResult<SeriesRow> {
    let mut rows: Vec<PinRow> = Vec::new();
    for spec in &series.pins {
        rows.push(build_pin_row(cfg, store, spec, check_remote)?);
    }
    Ok(SeriesRow {
        name: series.name.to_string(),
        pins: rows,
    })
}

fn build_pin_row(
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    spec: &PinSpec,
    check_remote: bool,
) -> CliResult<PinRow> {
    let project = cfg
        .project(spec.project())
        .map_err(|e| CliError::Core(e.into()))?;
    let resolved: Option<Pin> = spec.resolve(store).ok();
    let snapshot_present = resolved
        .as_ref()
        .is_some_and(|p| store.has(&p.project, &p.tag));
    let upstream_tags_ahead = if check_remote {
        upstream_lead(project, resolved.as_ref().map(|p| &p.tag)).ok()
    } else {
        None
    };
    let note = build_note(spec, resolved.as_ref(), snapshot_present);
    Ok(PinRow {
        project: spec.project().clone(),
        pin: pin_display(spec),
        resolved_tag: resolved.map(|p| p.tag),
        snapshot_present,
        upstream_tags_ahead,
        note,
    })
}

fn pin_display(spec: &PinSpec) -> PinDisplay {
    match spec {
        PinSpec::Literal { tag, .. } => PinDisplay::Literal {
            tag: tag.to_string(),
        },
        PinSpec::Pattern {
            pattern, select, ..
        } => {
            let label = match select {
                PinSelect::Latest => "latest",
                PinSelect::Oldest => "oldest",
            };
            PinDisplay::Pattern {
                pattern: pattern.to_string(),
                select: label.to_owned(),
            }
        }
        PinSpec::SelfRef { git_ref, .. } => PinDisplay::SelfRef {
            git_ref: git_ref.to_string(),
        },
    }
}

fn build_note(spec: &PinSpec, resolved: Option<&Pin>, present: bool) -> Option<String> {
    match (spec, resolved, present) {
        (PinSpec::Literal { project, tag }, _, false) => Some(format!(
            "run: backhopper snapshots generate --project {project} --since {tag}"
        )),
        (PinSpec::Pattern { project, .. }, None, _) => Some(format!(
            "pattern matched no snapshot tag: run `backhopper snapshots generate --project {project}`",
        )),
        (PinSpec::Pattern { project, .. }, Some(pin), false) => Some(format!(
            "resolved tag {} for {project} has no snapshot file on disk: rerun `backhopper snapshots rebuild --project {project} --tag {}`",
            pin.tag, pin.tag
        )),
        (
            PinSpec::SelfRef {
                project, git_ref, ..
            },
            _,
            _,
        ) => Some(format!(
            "self-project {project}@{git_ref}: snapshot is materialized on demand by `backhopper check`"
        )),
        _ => None,
    }
}

fn upstream_lead(project: &Project, resolved: Option<&TagName>) -> Result<usize, CliError> {
    let repo = GitRepo::open(
        project
            .require_git_url()
            .map_err(|e| CliError::Core(e.into()))?
            .to_path_buf(),
    )
    .map_err(CliError::Git)?;
    let listing = repo.list_tag_refs().map_err(CliError::Git)?;
    let filtered = filter_tags_for_project(listing.tags, project, None);
    Ok(count_newer_tags(&filtered, resolved))
}

pub fn count_newer_tags(filtered: &[TagName], resolved: Option<&TagName>) -> usize {
    let Some(resolved) = resolved else {
        return filtered.len();
    };
    filtered
        .iter()
        .filter(|t| version_cmp(t.as_str(), resolved.as_str()) == Ordering::Less)
        .count()
}

fn unpinned_projects(cfg: &Config) -> Vec<String> {
    let mut pinned: BTreeSet<&str> = BTreeSet::new();
    for s in &cfg.series {
        for p in &s.pins {
            pinned.insert(p.project().as_str());
        }
    }
    let mut out: Vec<String> = cfg
        .projects
        .iter()
        .map(|p| p.name.as_str().to_owned())
        .filter(|n| !pinned.contains(n.as_str()))
        .collect();
    out.sort();
    out
}

#[derive(Tabled)]
struct DoctorRow {
    series: String,
    project: String,
    pin: String,
    resolved: String,
    snapshot: String,
    upstream: String,
    note: String,
}

fn render_text(w: &mut dyn Write, payload: &DoctorPayload, style: TableStyle) -> CliResult<()> {
    writeln!(w, "config: {}", payload.config_path)?;
    writeln!(w, "snapshots: {}", payload.snapshot_dir)?;
    writeln!(
        w,
        "summary: {} series, {}/{} pin(s) covered, {} missing",
        payload.totals.series, payload.totals.covered, payload.totals.pins, payload.totals.missing,
    )?;
    writeln!(
        w,
        "caches: {} entr{}, {} bytes{}",
        payload.cache.entries,
        if payload.cache.entries == 1 {
            "y"
        } else {
            "ies"
        },
        payload.cache.bytes,
        if payload.cache.bytes > CACHE_SIZE_HINT_BYTES {
            " (consider `backhopper cache prune --older-than <DAYS>`)"
        } else {
            ""
        },
    )?;
    if payload.series.is_empty() {
        writeln!(
            w,
            "(no [[series]] in config; add one, or run `backhopper init --rabbitmq-repo-dir-path <PATH>` to seed)"
        )?;
    } else {
        let rows: Vec<DoctorRow> = payload
            .series
            .iter()
            .flat_map(|s| s.pins.iter().map(|p| doctor_row(s, p)))
            .collect();
        let mut table = Table::new(rows);
        style.apply(&mut table);
        writeln!(w)?;
        writeln!(w, "{table}")?;
    }
    if !payload.unpinned_projects.is_empty() {
        writeln!(w)?;
        writeln!(
            w,
            "configured projects not pinned by any series: {}",
            payload.unpinned_projects.join(", ")
        )?;
    }
    if payload.totals.missing > 0 {
        writeln!(w)?;
        writeln!(
            w,
            "next: run the per-row `note` commands, or rerun `check` with `--auto-generate`",
        )?;
    }
    Ok(())
}

fn doctor_row(series: &SeriesRow, pin: &PinRow) -> DoctorRow {
    let pin_label = match &pin.pin {
        PinDisplay::Literal { tag } => tag.clone(),
        PinDisplay::Pattern { pattern, select } => format!("{pattern} ({select})"),
        PinDisplay::SelfRef { git_ref } => format!("{git_ref} (self)"),
    };
    let resolved = pin
        .resolved_tag
        .as_ref()
        .map(TagName::to_string)
        .unwrap_or_else(|| "-".into());
    let snapshot = if pin.snapshot_present {
        "ok"
    } else {
        "MISSING"
    };
    let upstream = pin
        .upstream_tags_ahead
        .map(|n| {
            if n == 0 {
                "up to date".to_owned()
            } else {
                format!("{n} new tag(s)")
            }
        })
        .unwrap_or_else(|| "-".into());
    let note = pin.note.clone().unwrap_or_default();
    DoctorRow {
        series: series.name.clone(),
        project: pin.project.to_string(),
        pin: pin_label,
        resolved,
        snapshot: snapshot.into(),
        upstream,
        note,
    }
}
