// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use backhopper_core::config::{Config, Project};
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{SeriesName, TagName};

use crate::cli::{GlobalArgs, SeriesCmd};
use crate::commands::context::load_config;
use crate::commands::rabbitmq_components::{
    DepPin, DepSource, parse_components_mk, version_to_tag,
};
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
        SeriesCmd::Sync {
            from_branch,
            repo_dir_path,
            series_name,
        } => sync(args, &cfg, &from_branch, &repo_dir_path, series_name),
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

fn sync(
    args: &GlobalArgs,
    cfg: &Config,
    from_branch: &str,
    repo_dir_path: &Path,
    series_name: SeriesName,
) -> CliResult<i32> {
    let repo = GitRepo::open(repo_dir_path).map_err(|e| CliError::Core(e.into()))?;
    let commit = repo
        .resolve_rev(from_branch)
        .map_err(|e| CliError::Core(e.into()))?;
    let blobs = repo
        .read_paths_at_commit(&commit, |p| p == COMPONENTS_MK_PATH)
        .map_err(|e| CliError::Core(e.into()))?;
    let Some(blob) = blobs.into_iter().next() else {
        return Err(CliError::InvalidInput(format!(
            "{COMPONENTS_MK_PATH} not found at {from_branch} ({commit})",
        )));
    };
    let text = std::str::from_utf8(&blob.bytes).map_err(|e| {
        CliError::InvalidInput(format!(
            "{COMPONENTS_MK_PATH} at {from_branch} is not UTF-8: {e}"
        ))
    })?;
    let payload = build_sync_output(text, &series_name, &cfg.projects)?;
    let ctx = OutputContext::new(args.formatter, "series sync");
    render(&ctx, &payload, |w| {
        render_sync_text(&payload, w).map_err(CliError::from)
    })?;
    Ok(0)
}

fn pin_payload_for(pin: &DepPin, project: &Project) -> CliResult<PinPayload> {
    let raw_tag = match pin.source {
        DepSource::Hex => version_to_tag(&pin.version, &project.tag_prefix),
        DepSource::Git | DepSource::GitRmq => pin.version.clone(),
    };
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
        pins: s
            .pins
            .iter()
            .map(|p| PinPayload {
                project: p.project.to_string(),
                tag: p.tag.to_string(),
            })
            .collect(),
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
