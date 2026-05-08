use serde::Serialize;

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
        SeriesCmd::List => {
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
        SeriesCmd::Show { series } => {
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
        SeriesCmd::InferFromRabbitmq { repo, branch } => Err(CliError::Other(format!(
            "series infer-from-rabbitmq not implemented yet (repo={}, branch={})",
            repo.display(),
            branch
        ))),
    }
}
