// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Write as _};
use std::fs;
use std::io::{self, Write};

use serde::Serialize;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};

use backhopper_core::config::{Config, Project};
use backhopper_core::git::{GitRepo, unified_diff_body};
use backhopper_core::model::names::{ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::{PinSelect, PinSpec};

use crate::cli::{GlobalArgs, SeriesCmd, SyncCmd, SyncCommon};
use crate::commands::context::load_config;
use crate::commands::rabbitmq_components::{DepPin, dep_to_tag, parse_components_mk};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

const COMPONENTS_MK_PATH: &str = "rabbitmq-components.mk";

#[derive(Debug, Serialize)]
struct SeriesListEntry {
    name: String,
    pins: usize,
}

#[derive(Debug, Serialize)]
struct SeriesShow {
    name: String,
    pins: Vec<PinPayload>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct PinPayload {
    pub project: String,
    pub tag: String,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct SyncOutput {
    pub name: String,
    pub pins: Vec<PinPayload>,
    pub dropped_unconfigured: Vec<String>,
}

#[derive(Debug, Serialize, Clone, Default, PartialEq, Eq)]
pub struct MergeOutcome {
    pub added: Vec<PinPayload>,
    pub updated: Vec<MergeConflict>,
    pub skipped_conflicts: Vec<MergeConflict>,
    pub skipped_non_literal: Vec<ProjectName>,
    pub unchanged: Vec<PinPayload>,
    pub preserved: Vec<PinPayload>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct MergeConflict {
    pub project: ProjectName,
    pub existing_tag: TagName,
    pub inferred_tag: TagName,
}

#[derive(Debug, Serialize)]
struct MergeReport<'a> {
    series: &'a str,
    outcome: &'a MergeOutcome,
    dropped_unconfigured: &'a [String],
    config_path: String,
}

#[derive(Debug, Serialize)]
struct DiffReport<'a> {
    series: &'a str,
    mode: &'static str,
    diff: &'a str,
    outcome: Option<&'a MergeOutcome>,
    dropped_unconfigured: &'a [String],
}

pub fn build_sync_output(
    components_mk: &str,
    series_name: &SeriesName,
    projects: &[Project],
) -> CliResult<SyncOutput> {
    let pins = parse_components_mk(components_mk);
    let configured: BTreeMap<&str, &Project> =
        projects.iter().map(|p| (p.name.as_str(), p)).collect();
    let mut kept: Vec<PinPayload> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for pin in &pins {
        match configured.get(pin.name.as_str()) {
            Some(project) => kept.push(pin_payload_for(pin, project)?),
            None => dropped.push(pin.name.clone()),
        }
    }
    kept.sort_by(|a, b| a.project.cmp(&b.project));
    dropped.sort();
    Ok(SyncOutput {
        name: series_name.to_string(),
        pins: kept,
        dropped_unconfigured: dropped,
    })
}

pub fn render_sync_text<W: Write + ?Sized>(payload: &SyncOutput, w: &mut W) -> io::Result<()> {
    writeln!(w, "[[series]]")?;
    writeln!(w, "name = \"{}\"", payload.name)?;
    if payload.pins.is_empty() {
        writeln!(w, "pins = []")?;
    } else {
        writeln!(w, "pins = [")?;
        let widest = payload
            .pins
            .iter()
            .map(|p| p.project.len())
            .max()
            .unwrap_or(0);
        for p in &payload.pins {
            writeln!(
                w,
                "    {{ project = \"{}\",{:gap$} tag = \"{}\" }},",
                p.project,
                "",
                p.tag,
                gap = widest - p.project.len(),
            )?;
        }
        writeln!(w, "]")?;
    }
    if !payload.dropped_unconfigured.is_empty() {
        writeln!(w)?;
        writeln!(
            w,
            "# dropped (no matching [[project]] in this backhopper.toml):"
        )?;
        for name in &payload.dropped_unconfigured {
            writeln!(w, "#   {name}")?;
        }
    }
    Ok(())
}

pub fn handle(args: &GlobalArgs, cmd: SeriesCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        SeriesCmd::List => list(args, &cfg),
        SeriesCmd::Show { series } => show(args, &cfg, series),
        SeriesCmd::Sync(sync) => sync_dispatch(args, &cfg, sync),
    }
}

fn list(args: &GlobalArgs, cfg: &Config) -> CliResult<i32> {
    let entries: Vec<_> = cfg
        .series
        .iter()
        .map(|s| SeriesListEntry {
            name: s.name.to_string(),
            pins: s.pins.len(),
        })
        .collect();
    let ctx = OutputContext::new(args.formatter, "series list");
    render(&ctx, &entries, |w| {
        for e in &entries {
            writeln!(w, "{}\t{} pins", e.name, e.pins)?;
        }
        Ok(())
    })?;
    Ok(0)
}

fn sync_dispatch(args: &GlobalArgs, cfg: &Config, cmd: SyncCmd) -> CliResult<i32> {
    match cmd {
        SyncCmd::Preview { common } => sync_preview(args, cfg, common),
        SyncCmd::Diff {
            common,
            replace,
            overwrite_existing,
        } => sync_diff(args, cfg, common, replace, overwrite_existing),
        SyncCmd::Merge {
            common,
            overwrite_existing,
        } => sync_merge(args, cfg, common, overwrite_existing),
        SyncCmd::Replace { common } => sync_replace(args, cfg, common),
    }
}

fn build_payload(cfg: &Config, common: &SyncCommon) -> CliResult<SyncOutput> {
    let repo = GitRepo::open(&common.repo_dir_path).map_err(|e| CliError::Core(e.into()))?;
    let commit = repo
        .resolve_rev(&common.from_branch)
        .map_err(|e| CliError::Core(e.into()))?;
    let blobs = repo
        .read_paths_at_commit(&commit, |p| p == COMPONENTS_MK_PATH)
        .map_err(|e| CliError::Core(e.into()))?;
    let Some(blob) = blobs.into_iter().next() else {
        return Err(CliError::InvalidInput(format!(
            "{COMPONENTS_MK_PATH} not found at {} ({commit})",
            common.from_branch,
        )));
    };
    let text = std::str::from_utf8(&blob.bytes).map_err(|e| {
        CliError::InvalidInput(format!(
            "{COMPONENTS_MK_PATH} at {} is not UTF-8: {e}",
            common.from_branch,
        ))
    })?;
    build_sync_output(text, &common.series_name, &cfg.projects)
}

fn sync_preview(args: &GlobalArgs, cfg: &Config, common: SyncCommon) -> CliResult<i32> {
    let payload = build_payload(cfg, &common)?;
    let ctx = OutputContext::new(args.formatter, "series sync preview");
    render(&ctx, &payload, |w| {
        render_sync_text(&payload, w).map_err(CliError::from)
    })?;
    Ok(0)
}

fn sync_replace(args: &GlobalArgs, cfg: &Config, common: SyncCommon) -> CliResult<i32> {
    let payload = build_payload(cfg, &common)?;
    let path = &cfg.config_path;
    let existing = fs::read_to_string(path).map_err(CliError::Io)?;
    let updated = replace_series_block(&existing, &payload)?;
    fs::write(path, updated).map_err(CliError::Io)?;
    let ctx = OutputContext::new(args.formatter, "series sync replace");
    render(&ctx, &payload, |w| {
        writeln!(
            w,
            "replaced series {} with {} pins",
            payload.name,
            payload.pins.len()
        )?;
        writeln!(w, "wrote {}", path.display())?;
        Ok(())
    })?;
    Ok(0)
}

fn sync_merge(
    args: &GlobalArgs,
    cfg: &Config,
    common: SyncCommon,
    overwrite_existing: bool,
) -> CliResult<i32> {
    let payload = build_payload(cfg, &common)?;
    let path = &cfg.config_path;
    let existing = fs::read_to_string(path).map_err(CliError::Io)?;
    let (updated, outcome) = merge_sync_into_config_text(&existing, &payload, overwrite_existing)?;
    fs::write(path, updated).map_err(CliError::Io)?;
    let report = MergeReport {
        series: &payload.name,
        outcome: &outcome,
        dropped_unconfigured: &payload.dropped_unconfigured,
        config_path: path.display().to_string(),
    };
    let ctx = OutputContext::new(args.formatter, "series sync merge");
    render(&ctx, &report, |w| {
        render_merge_report_text(&report, w).map_err(CliError::from)
    })?;
    Ok(0)
}

fn sync_diff(
    args: &GlobalArgs,
    cfg: &Config,
    common: SyncCommon,
    replace: bool,
    overwrite_existing: bool,
) -> CliResult<i32> {
    let payload = build_payload(cfg, &common)?;
    let path = &cfg.config_path;
    let existing = fs::read_to_string(path).map_err(CliError::Io)?;
    let (mode_label, proposed, outcome) = if replace {
        let proposed = replace_series_block(&existing, &payload)?;
        ("replace", proposed, None)
    } else {
        let (proposed, outcome) =
            merge_sync_into_config_text(&existing, &payload, overwrite_existing)?;
        ("merge", proposed, Some(outcome))
    };
    let diff = unified_diff(&existing, &proposed, path.display());
    let report = DiffReport {
        series: &payload.name,
        mode: mode_label,
        diff: &diff,
        outcome: outcome.as_ref(),
        dropped_unconfigured: &payload.dropped_unconfigured,
    };
    let ctx = OutputContext::new(args.formatter, "series sync diff");
    render(&ctx, &report, |w| {
        render_diff_report_text(&report, w).map_err(CliError::from)
    })?;
    Ok(0)
}

pub fn replace_series_block(existing: &str, payload: &SyncOutput) -> CliResult<String> {
    let mut doc: DocumentMut = parse_doc(existing)?;
    let series_array = ensure_series_array(&mut doc)?;
    let new_block = build_series_table(payload);
    let mut replaced = false;
    for table in series_array.iter_mut() {
        if table_name(table) == Some(payload.name.as_str()) {
            *table = new_block.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        series_array.push(new_block);
    }
    Ok(doc.to_string())
}

pub fn merge_sync_into_config_text(
    existing: &str,
    payload: &SyncOutput,
    overwrite_existing: bool,
) -> CliResult<(String, MergeOutcome)> {
    let mut doc: DocumentMut = parse_doc(existing)?;
    let series_array = ensure_series_array(&mut doc)?;
    let existing_idx = series_array
        .iter()
        .position(|t| table_name(t) == Some(payload.name.as_str()));

    let Some(idx) = existing_idx else {
        let outcome = MergeOutcome {
            added: payload.pins.clone(),
            ..MergeOutcome::default()
        };
        series_array.push(build_series_table(payload));
        return Ok((doc.to_string(), outcome));
    };

    let table_ref = series_array.get(idx).expect("index from position()");
    let existing_pins = read_pins(table_ref);
    let all_existing_projects = read_pin_projects(table_ref);
    let inferred_by_project: BTreeMap<&str, &str> = payload
        .pins
        .iter()
        .map(|p| (p.project.as_str(), p.tag.as_str()))
        .collect();
    let existing_by_project: BTreeMap<&str, &str> = existing_pins
        .iter()
        .map(|p| (p.project.as_str(), p.tag.as_str()))
        .collect();

    let mut outcome = MergeOutcome::default();
    for pin in &payload.pins {
        match existing_by_project.get(pin.project.as_str()) {
            None if all_existing_projects.contains(pin.project.as_str()) => {
                outcome.skipped_non_literal.push(
                    ProjectName::new(pin.project.clone()).map_err(|e| CliError::Core(e.into()))?,
                );
            }
            None => outcome.added.push(pin.clone()),
            Some(t) if *t == pin.tag.as_str() => outcome.unchanged.push(pin.clone()),
            Some(t) => {
                let conflict = MergeConflict {
                    project: ProjectName::new(pin.project.clone())
                        .map_err(|e| CliError::Core(e.into()))?,
                    existing_tag: TagName::new((*t).to_owned())
                        .map_err(|e| CliError::Core(e.into()))?,
                    inferred_tag: TagName::new(pin.tag.clone())
                        .map_err(|e| CliError::Core(e.into()))?,
                };
                if overwrite_existing {
                    outcome.updated.push(conflict);
                } else {
                    outcome.skipped_conflicts.push(conflict);
                }
            }
        }
    }
    for pin in &existing_pins {
        if !inferred_by_project.contains_key(pin.project.as_str()) {
            outcome.preserved.push(pin.clone());
        }
    }

    if outcome.added.is_empty() && outcome.updated.is_empty() {
        return Ok((doc.to_string(), outcome));
    }

    let table = series_array.get_mut(idx).expect("index from position()");
    apply_in_place(table, &outcome.added, &outcome.updated)?;
    Ok((doc.to_string(), outcome))
}

fn apply_in_place(
    table: &mut Table,
    added: &[PinPayload],
    updated: &[MergeConflict],
) -> CliResult<()> {
    let entry = table
        .entry("pins")
        .or_insert(Item::Value(Value::Array(Array::new())));
    let Some(pins) = entry.as_array_mut() else {
        return Err(CliError::InvalidInput(
            "series 'pins' is not an array".into(),
        ));
    };
    for c in updated {
        for entry in pins.iter_mut() {
            let Some(inline) = entry.as_inline_table_mut() else {
                continue;
            };
            let matches = inline
                .get("project")
                .and_then(|v| v.as_str())
                .is_some_and(|p| p == c.project.as_str());
            if matches {
                inline.insert("tag", c.inferred_tag.as_str().into());
                break;
            }
        }
    }
    let prefix = pins
        .iter()
        .last()
        .and_then(|v| v.decor().prefix())
        .and_then(|r| r.as_str())
        .map(str::to_owned);
    for pin in added {
        let mut inline = InlineTable::new();
        inline.insert("project", pin.project.as_str().into());
        inline.insert("tag", pin.tag.as_str().into());
        let mut v = Value::InlineTable(inline);
        if let Some(p) = &prefix {
            v.decor_mut().set_prefix(p.as_str());
        }
        pins.push_formatted(v);
    }
    Ok(())
}

fn parse_doc(existing: &str) -> CliResult<DocumentMut> {
    existing.parse().map_err(|e: toml_edit::TomlError| {
        CliError::InvalidInput(format!("invalid TOML in config: {e}"))
    })
}

fn ensure_series_array(doc: &mut DocumentMut) -> CliResult<&mut ArrayOfTables> {
    doc.entry("series")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            CliError::InvalidInput("config 'series' key is not an array of tables".into())
        })
}

fn table_name(table: &Table) -> Option<&str> {
    table.get("name").and_then(|v| v.as_str())
}

fn read_pin_projects(table: &Table) -> BTreeSet<String> {
    table
        .get("pins")
        .and_then(|v| v.as_array())
        .into_iter()
        .flat_map(|a| a.iter())
        .filter_map(|v| v.as_inline_table())
        .filter_map(|t| t.get("project").and_then(|v| v.as_str()))
        .map(str::to_owned)
        .collect()
}

fn read_pins(table: &Table) -> Vec<PinPayload> {
    let Some(pins) = table.get("pins").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for value in pins.iter() {
        let Some(inline) = value.as_inline_table() else {
            continue;
        };
        let project = inline.get("project").and_then(|v| v.as_str());
        let tag = inline.get("tag").and_then(|v| v.as_str());
        if let (Some(project), Some(tag)) = (project, tag) {
            out.push(PinPayload {
                project: project.to_owned(),
                tag: tag.to_owned(),
            });
        }
    }
    out
}

fn build_series_table(payload: &SyncOutput) -> Table {
    let mut table = Table::new();
    table["name"] = value(payload.name.as_str());
    let mut pins = Array::new();
    let mut sorted = payload.pins.clone();
    sorted.sort_by(|a, b| a.project.cmp(&b.project));
    for pin in &sorted {
        let mut inline = InlineTable::new();
        inline.insert("project", pin.project.as_str().into());
        inline.insert("tag", pin.tag.as_str().into());
        pins.push(Value::InlineTable(inline));
    }
    table["pins"] = value(pins);
    table
}

pub fn unified_diff(before: &str, after: &str, label: impl Display) -> String {
    let body = unified_diff_body(before, after);
    if body.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let _ = writeln!(out, "--- a/{label}");
    let _ = writeln!(out, "+++ b/{label}");
    out.push_str(&body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn render_merge_report_text<W: Write + ?Sized>(
    report: &MergeReport<'_>,
    w: &mut W,
) -> io::Result<()> {
    let o = report.outcome;
    writeln!(w, "merged into series {}", report.series)?;
    writeln!(
        w,
        "  added: {}  updated: {}  unchanged: {}  conflicts skipped: {}  non-literal skipped: {}  preserved: {}",
        o.added.len(),
        o.updated.len(),
        o.unchanged.len(),
        o.skipped_conflicts.len(),
        o.skipped_non_literal.len(),
        o.preserved.len(),
    )?;
    for p in &o.added {
        writeln!(w, "  + {} {}", p.project, p.tag)?;
    }
    for c in &o.updated {
        writeln!(
            w,
            "  ~ {} {} -> {}",
            c.project, c.existing_tag, c.inferred_tag
        )?;
    }
    if !o.skipped_conflicts.is_empty() {
        for c in &o.skipped_conflicts {
            writeln!(
                w,
                "  ! {} {} -> {} (skipped)",
                c.project, c.existing_tag, c.inferred_tag
            )?;
        }
        writeln!(w, "  re-run with --overwrite-existing to apply")?;
    }
    for p in &o.skipped_non_literal {
        writeln!(w, "  ? {p} pinned by pattern; merge leaves it untouched")?;
    }
    if !report.dropped_unconfigured.is_empty() {
        writeln!(
            w,
            "  unconfigured (no matching [[project]]): {}",
            report.dropped_unconfigured.join(", ")
        )?;
    }
    writeln!(w, "wrote {}", report.config_path)?;
    Ok(())
}

fn render_diff_report_text<W: Write + ?Sized>(
    report: &DiffReport<'_>,
    w: &mut W,
) -> io::Result<()> {
    writeln!(
        w,
        "# series sync diff ({mode}) for {series}",
        mode = report.mode,
        series = report.series,
    )?;
    if let Some(o) = report.outcome {
        writeln!(
            w,
            "# added: {}  updated: {}  unchanged: {}  conflicts skipped: {}  non-literal skipped: {}  preserved: {}",
            o.added.len(),
            o.updated.len(),
            o.unchanged.len(),
            o.skipped_conflicts.len(),
            o.skipped_non_literal.len(),
            o.preserved.len(),
        )?;
        if !o.skipped_conflicts.is_empty() {
            for c in &o.skipped_conflicts {
                writeln!(
                    w,
                    "# conflict {} {} -> {} (skipped)",
                    c.project, c.existing_tag, c.inferred_tag,
                )?;
            }
            writeln!(w, "# re-run with --overwrite-existing to apply")?;
        }
        for p in &o.skipped_non_literal {
            writeln!(w, "# {p} pinned by pattern; merge leaves it untouched")?;
        }
    }
    if !report.dropped_unconfigured.is_empty() {
        writeln!(
            w,
            "# unconfigured (no matching [[project]]): {}",
            report.dropped_unconfigured.join(", "),
        )?;
    }
    if report.diff.is_empty() {
        writeln!(w, "# no change")?;
    } else {
        write!(w, "{}", report.diff)?;
    }
    Ok(())
}

fn pin_spec_to_payload(spec: &PinSpec) -> PinPayload {
    match spec {
        PinSpec::Literal { project, tag } => PinPayload {
            project: project.to_string(),
            tag: tag.to_string(),
        },
        PinSpec::Pattern {
            project,
            pattern,
            select,
        } => {
            let select_label = match select {
                PinSelect::Latest => "latest",
                PinSelect::Oldest => "oldest",
            };
            PinPayload {
                project: project.to_string(),
                tag: format!("{pattern} ({select_label})"),
            }
        }
    }
}

fn pin_payload_for(pin: &DepPin, project: &Project) -> CliResult<PinPayload> {
    let raw_tag = dep_to_tag(pin, &project.tag_prefix);
    let tag = TagName::new(raw_tag.clone()).map_err(|e| {
        CliError::InvalidInput(format!(
            "dep {} produced invalid tag {raw_tag:?}: {e}",
            pin.name
        ))
    })?;
    Ok(PinPayload {
        project: project.name.to_string(),
        tag: tag.to_string(),
    })
}

fn show(args: &GlobalArgs, cfg: &Config, series: SeriesName) -> CliResult<i32> {
    let s = cfg
        .series_by_name(&series)
        .map_err(|e| CliError::Core(e.into()))?;
    let payload = SeriesShow {
        name: s.name.to_string(),
        pins: s.pins.iter().map(pin_spec_to_payload).collect(),
    };
    let ctx = OutputContext::new(args.formatter, "series show");
    render(&ctx, &payload, |w| {
        writeln!(w, "{}", payload.name)?;
        for p in &payload.pins {
            writeln!(w, "  {} {}", p.project, p.tag)?;
        }
        Ok(())
    })?;
    Ok(0)
}
