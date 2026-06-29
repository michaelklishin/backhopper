// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use bel7_cli::{PARTIAL_SUCCESS_I32, TableStyle};
use serde::Serialize;
use tabled::Tabled;

use backhopper_cache::{WorkspaceCaches, stats};
use backhopper_core::config::{Config, Project, Series};
use backhopper_core::model::names::{ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::{Pin, PinSelect, PinSpec};
use backhopper_core::store::{ReadOnly, SnapshotStore};
use backhopper_core::versions::version_cmp;

use backhopper_git::GitRepo;

use crate::cli::{DoctorCmd, GlobalArgs};
use crate::commands::auto_generate::snapshot_generate_command;
use crate::commands::context::{load_config, open_store_read, snapshot_dir};
use crate::commands::pin_coverage::{PinCoverage, classify_pin};
use crate::commands::snapshots::filter_tags_for_project;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};
use crate::tables::styled_table;

#[derive(Debug, Serialize)]
struct DoctorPayload {
    config_path: String,
    snapshot_dir: String,
    series: Vec<SeriesRow>,
    totals: Totals,
    unpinned_projects: Vec<String>,
    /// Tracked projects a series carries no pin for: the symbol
    /// lookup cannot miss against a pin that is not there, so the
    /// verdict silently reverts to vacuous. Advisory only.
    series_pin_gaps: Vec<SeriesPinGap>,
    cache: CacheRow,
}

#[derive(Debug, Clone, Serialize)]
struct SeriesPinGap {
    series: SeriesName,
    project: ProjectName,
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
    /// Newest store tag for the project by version order. `None` for
    /// self pins, which resolve against a git ref rather than a tag.
    store_newest_tag: Option<TagName>,
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
    // one store listing per project, however many series pin it
    let mut store_tags: BTreeMap<ProjectName, Vec<TagName>> = BTreeMap::new();
    for series in &cfg.series {
        if let Some(filter) = &cmd.series
            && !series.name.as_str().contains(filter.as_str())
        {
            continue;
        }
        let row = build_series_row(cfg, store, series, cmd.check_remote, &mut store_tags)?;
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
        series_pin_gaps: series_pin_gaps(cfg, cmd.series.as_deref()),
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
    store_tags: &mut BTreeMap<ProjectName, Vec<TagName>>,
) -> CliResult<SeriesRow> {
    let mut rows: Vec<PinRow> = Vec::new();
    for spec in &series.pins {
        rows.push(build_pin_row(cfg, store, spec, check_remote, store_tags)?);
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
    store_tags: &mut BTreeMap<ProjectName, Vec<TagName>>,
) -> CliResult<PinRow> {
    let project = cfg.project(spec.project())?;
    let coverage = classify_pin(spec, store);
    let resolved: Option<Pin> = match &coverage {
        PinCoverage::Resolved { pin, .. } => Some(pin.clone()),
        PinCoverage::Unresolved { .. } | PinCoverage::SelfPin => None,
    };
    let snapshot_present = matches!(coverage, PinCoverage::Resolved { present: true, .. });
    let store_newest_tag = match spec {
        PinSpec::SelfRef { .. } => None,
        _ => {
            let tags = store_tags
                .entry(spec.project().clone())
                .or_insert_with(|| store.list_tags(spec.project()).unwrap_or_default());
            PinSelect::Latest.pick(tags.iter()).cloned()
        }
    };
    let upstream_tags_ahead = if check_remote {
        upstream_lead(project, resolved.as_ref().map(|p| &p.tag)).ok()
    } else {
        None
    };
    let note = build_note(spec, resolved.as_ref(), snapshot_present)
        .or_else(|| staleness_note(spec, resolved.as_ref(), store_newest_tag.as_ref()));
    Ok(PinRow {
        project: spec.project().clone(),
        pin: pin_display(spec),
        resolved_tag: resolved.map(|p| p.tag),
        snapshot_present,
        store_newest_tag,
        upstream_tags_ahead,
        note,
    })
}

/// The store knowing a newer tag than the pin resolves to is the
/// post-cascade window between a landed bump and `series sync`.
pub fn staleness_note(
    spec: &PinSpec,
    resolved: Option<&Pin>,
    newest: Option<&TagName>,
) -> Option<String> {
    let resolved = resolved?;
    let newest = newest?;
    if version_cmp(newest.as_str(), resolved.tag.as_str()) != Ordering::Less {
        return None;
    }
    match spec {
        PinSpec::Literal { .. } => Some(format!(
            "store has a newer snapshot ({newest}): if the series branch landed a pin bump, run `backhopper series sync diff`"
        )),
        // Latest-pattern pins resolve to the newest *matching* tag, so
        // a newer overall tag means the glob is what excludes it
        PinSpec::Pattern { .. } => Some(format!("pin pattern excludes newer tag {newest}")),
        PinSpec::SelfRef { .. } => None,
    }
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
        (PinSpec::Literal { project, tag }, _, false) => {
            Some(format!("run: {}", snapshot_generate_command(project, tag)))
        }
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
    let repo = GitRepo::open(project.require_git_url()?.to_path_buf())?;
    let listing = repo.list_tag_refs()?;
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

/// Per-series coverage: every tracked project either pins in the
/// series or sits on the series' `untracked_projects` opt-out.
fn series_pin_gaps(cfg: &Config, filter: Option<&str>) -> Vec<SeriesPinGap> {
    let mut gaps = Vec::new();
    for series in &cfg.series {
        if let Some(filter) = filter
            && !series.name.as_str().contains(filter)
        {
            continue;
        }
        let pinned: BTreeSet<&ProjectName> = series.pins.iter().map(|p| p.project()).collect();
        for project in &cfg.projects {
            if pinned.contains(&project.name) || series.untracked_projects.contains(&project.name) {
                continue;
            }
            gaps.push(SeriesPinGap {
                series: series.name.clone(),
                project: project.name.clone(),
            });
        }
    }
    gaps
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
    newest: String,
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
        writeln!(w)?;
        writeln!(w, "{}", styled_table(rows, style))?;
    }
    if !payload.series_pin_gaps.is_empty() {
        writeln!(w)?;
        for gap in &payload.series_pin_gaps {
            writeln!(
                w,
                "series {}: no pin for tracked project {} (adopt via series sync merge --series-name {}, or list it under untracked_projects)",
                gap.series, gap.project, gap.series
            )?;
        }
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
    let newest = pin
        .store_newest_tag
        .as_ref()
        .map(TagName::to_string)
        .unwrap_or_else(|| "-".into());
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
        newest,
        upstream,
        note,
    }
}
