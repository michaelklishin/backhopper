use serde::Serialize;

use backhopper_core::config::Config;
use backhopper_core::model::names::SeriesName;

use crate::cli::{GlobalArgs, SeriesCmd};
use crate::commands::context::load_config;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

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

#[derive(Debug, Serialize)]
struct PinPayload {
    project: String,
    tag: String,
}

pub fn handle(args: &GlobalArgs, cmd: SeriesCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        SeriesCmd::List => list(args, &cfg),
        SeriesCmd::Show { series } => show(args, &cfg, series),
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
