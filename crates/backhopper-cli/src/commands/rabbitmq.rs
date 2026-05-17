use std::io::{self, Write};
use std::path::Path;
use std::str;

use serde::Serialize;

use backhopper_core::config::Config;
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{CommitSha, ProjectName, TagName};

use crate::cli::{GlobalArgs, RabbitmqCmd};
use crate::commands::context::load_config;
use crate::commands::rabbitmq_components::{
    DepPin, DepSource, parse_components_mk, series_name_for_branch, version_to_tag,
};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

const COMPONENTS_MK: &str = "rabbitmq-components.mk";

#[derive(Debug, Serialize)]
struct PinPayload {
    project: String,
    tag: String,
}

#[derive(Debug, Serialize)]
struct InferredSeries {
    name: String,
    branch: String,
    pins: Vec<PinPayload>,
    skipped: Vec<SkippedPin>,
}

#[derive(Debug, Serialize)]
struct SkippedPin {
    name: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct InferOutput {
    series: Vec<InferredSeries>,
}

pub fn handle(args: &GlobalArgs, cmd: RabbitmqCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        RabbitmqCmd::InferSeries {
            repo_dir_path,
            branch,
            all_branches,
            branches,
            name,
            show_skipped,
        } => infer(
            args,
            &cfg,
            &repo_dir_path,
            branch,
            all_branches,
            &branches,
            name,
            show_skipped,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn infer(
    args: &GlobalArgs,
    cfg: &Config,
    repo: &Path,
    branch: Option<String>,
    all_branches: bool,
    branches: &[String],
    explicit_name: Option<String>,
    show_skipped: bool,
) -> CliResult<i32> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let targets: Vec<String> = if all_branches {
        branches.to_vec()
    } else {
        vec![branch.unwrap_or_else(|| "main".into())]
    };
    let mut output = InferOutput { series: Vec::new() };
    for branch_ref in targets {
        match infer_one(&g, cfg, &branch_ref, explicit_name.as_deref()) {
            Ok(s) => output.series.push(s),
            Err(e) => {
                if show_skipped {
                    eprintln!("warning: skipping branch {}: {}", branch_ref, e);
                }
            }
        }
    }
    if output.series.is_empty() {
        return Err(CliError::InvalidInput(
            "no branches yielded a parseable rabbitmq-components.mk".into(),
        ));
    }
    let ctx = OutputContext::new(args.formatter, "rabbitmq infer_series");
    render(&ctx, &output, |w| {
        for s in &output.series {
            render_series_toml(w, s)?;
            if show_skipped {
                for sk in &s.skipped {
                    writeln!(w, "# skipped {}: {}", sk.name, sk.reason)?;
                }
            }
            writeln!(w)?;
        }
        Ok(())
    })?;
    Ok(0)
}

fn infer_one(
    g: &GitRepo,
    cfg: &Config,
    branch: &str,
    explicit_name: Option<&str>,
) -> Result<InferredSeries, String> {
    let commit = resolve_branch(g, branch)?;
    let blobs = g
        .read_paths_at_commit(&commit, |p| p == COMPONENTS_MK)
        .map_err(|e| e.to_string())?;
    let blob = blobs
        .into_iter()
        .next()
        .ok_or_else(|| format!("{} not present at {}", COMPONENTS_MK, branch))?;
    let text = str::from_utf8(&blob.bytes).map_err(|e| e.to_string())?;
    let pins = parse_components_mk(text);
    let mut kept: Vec<PinPayload> = Vec::new();
    let mut skipped: Vec<SkippedPin> = Vec::new();
    for pin in pins {
        let Ok(project_name) = ProjectName::new(pin.name.clone()) else {
            skipped.push(SkippedPin {
                name: pin.name.clone(),
                reason: "name not a valid project identifier".into(),
            });
            continue;
        };
        let Ok(project) = cfg.project(&project_name) else {
            continue;
        };
        let tag_str = canonicalize_pin(&pin, &project.tag_prefix);
        let Ok(tag) = TagName::new(tag_str.clone()) else {
            skipped.push(SkippedPin {
                name: pin.name.clone(),
                reason: format!("tag {:?} not a valid tag name", tag_str),
            });
            continue;
        };
        kept.push(PinPayload {
            project: project_name.to_string(),
            tag: tag.to_string(),
        });
    }
    let name = explicit_name
        .map(str::to_owned)
        .unwrap_or_else(|| series_name_for_branch(branch));
    Ok(InferredSeries {
        name,
        branch: branch.to_owned(),
        pins: kept,
        skipped,
    })
}

fn resolve_branch(g: &GitRepo, branch: &str) -> Result<CommitSha, String> {
    let candidates = [
        branch.to_owned(),
        format!("refs/heads/{}", branch),
        format!("refs/tags/{}", branch),
    ];
    for spec in candidates {
        if let Ok(tag_name) = TagName::new(spec.clone()) {
            if let Ok(sha) = g.resolve_tag(&tag_name) {
                return Ok(sha);
            }
        }
        if let Ok(sha) = g.resolve_rev(&spec) {
            return Ok(sha);
        }
    }
    Err(format!("could not resolve branch or rev {:?}", branch))
}

fn canonicalize_pin(pin: &DepPin, tag_prefix: &str) -> String {
    match pin.source {
        DepSource::Hex => version_to_tag(&pin.version, tag_prefix),
        DepSource::Git | DepSource::GitRmq => pin.version.clone(),
    }
}

fn render_series_toml(w: &mut dyn Write, s: &InferredSeries) -> io::Result<()> {
    writeln!(w, "[[series]]")?;
    writeln!(w, "name = {:?}", s.name)?;
    writeln!(w, "# inferred from {}", s.branch)?;
    writeln!(w, "pins = [")?;
    for p in &s.pins {
        writeln!(w, "  {{ project = {:?}, tag = {:?} }},", p.project, p.tag)?;
    }
    writeln!(w, "]")?;
    Ok(())
}
