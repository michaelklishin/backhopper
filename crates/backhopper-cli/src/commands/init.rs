// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str;

use serde::Serialize;

use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{ProjectName, SeriesName, TagName};

use crate::cli::{GlobalArgs, InitCmd};
use crate::commands::rabbitmq_components::{
    DepPin, dep_to_tag, parse_components_mk, series_name_for_branch,
};
use crate::commands::series::resolve_branch;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

const CONFIG_FILE_NAME: &str = "backhopper.toml";
const COMPONENTS_MK_PATH: &str = "rabbitmq-components.mk";
const DEFAULT_TAG_PREFIX: &str = "v";

#[derive(Debug, Serialize)]
struct InitPayload {
    wrote: PathBuf,
    snapshot_dir: PathBuf,
    projects: Vec<String>,
    series: Vec<String>,
    skipped_branches: Vec<SkippedBranch>,
}

#[derive(Debug, Serialize)]
pub struct SkippedBranch {
    pub branch: String,
    pub reason: String,
}

pub fn handle(args: &GlobalArgs, cmd: InitCmd) -> CliResult<i32> {
    let config_dir = resolve_config_dir(&cmd)?;
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    if config_path.exists() && !cmd.force {
        return Err(CliError::InvalidInput(format!(
            "{} already exists; pass --force to overwrite",
            config_path.display()
        )));
    }
    let snapshot_dir = resolve_snapshot_dir(args, &config_dir);
    fs::create_dir_all(&snapshot_dir).map_err(CliError::Io)?;

    let inferred = if let Some(repo_dir) = &cmd.rabbitmq_repo_dir_path {
        infer_from_rabbitmq(repo_dir, &cmd.rabbitmq_branches)?
    } else {
        InferredWorkspace::default()
    };
    let toml = build_toml(&snapshot_dir, &inferred);
    fs::write(&config_path, &toml).map_err(CliError::Io)?;

    let payload = InitPayload {
        wrote: config_path,
        snapshot_dir,
        projects: inferred.projects.keys().map(ToString::to_string).collect(),
        series: inferred.series.iter().map(|s| s.name.to_string()).collect(),
        skipped_branches: inferred.skipped,
    };
    let ctx = OutputContext::new(args.formatter, "init");
    render_with_exit(&ctx, &payload, 0, |w| render_text(w, &payload))
}

fn resolve_config_dir(cmd: &InitCmd) -> CliResult<PathBuf> {
    let dir = match &cmd.config_dir_path {
        Some(p) => p.clone(),
        None => env::current_dir().map_err(CliError::Io)?,
    };
    fs::create_dir_all(&dir).map_err(CliError::Io)?;
    Ok(absolute(&dir))
}

fn resolve_snapshot_dir(args: &GlobalArgs, config_dir: &Path) -> PathBuf {
    let p = args
        .snapshot_dir_path
        .clone()
        .unwrap_or_else(|| config_dir.join("snapshots"));
    absolute(&p)
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

#[derive(Debug, Default)]
pub struct InferredWorkspace {
    pub projects: BTreeMap<ProjectName, InferredProject>,
    pub series: Vec<InferredSeries>,
    pub skipped: Vec<SkippedBranch>,
}

#[derive(Debug)]
pub struct InferredProject {
    pub name: ProjectName,
    pub tag_prefix: String,
}

#[derive(Debug)]
pub struct InferredSeries {
    pub name: SeriesName,
    pub branch: String,
    pub pins: Vec<InferredPin>,
}

#[derive(Debug)]
pub struct InferredPin {
    pub project: ProjectName,
    pub tag: TagName,
}

fn infer_from_rabbitmq(repo_dir: &Path, branches: &[String]) -> CliResult<InferredWorkspace> {
    let repo = GitRepo::open(repo_dir.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let mut workspace = InferredWorkspace::default();
    for branch in branches {
        match read_components_at_branch(&repo, branch) {
            Ok(deps) => {
                let Ok(series_name) = SeriesName::new(series_name_for_branch(branch)) else {
                    workspace.skipped.push(SkippedBranch {
                        branch: branch.clone(),
                        reason: format!("derived series name for branch {branch:?} is not valid"),
                    });
                    continue;
                };
                let mut pins: Vec<InferredPin> = Vec::new();
                for dep in deps {
                    let Ok(project_name) = ProjectName::new(dep.name.clone()) else {
                        workspace.skipped.push(SkippedBranch {
                            branch: branch.clone(),
                            reason: format!("dep {:?}: not a valid project name", dep.name),
                        });
                        continue;
                    };
                    let raw_tag = dep_to_tag(&dep, DEFAULT_TAG_PREFIX);
                    let Ok(tag) = TagName::new(raw_tag.clone()) else {
                        workspace.skipped.push(SkippedBranch {
                            branch: branch.clone(),
                            reason: format!("dep {}: tag {raw_tag:?} not valid", dep.name),
                        });
                        continue;
                    };
                    workspace
                        .projects
                        .entry(project_name.clone())
                        .or_insert(InferredProject {
                            name: project_name.clone(),
                            tag_prefix: DEFAULT_TAG_PREFIX.to_owned(),
                        });
                    pins.push(InferredPin {
                        project: project_name,
                        tag,
                    });
                }
                workspace.series.push(InferredSeries {
                    name: series_name,
                    branch: branch.clone(),
                    pins,
                });
            }
            Err(reason) => workspace.skipped.push(SkippedBranch {
                branch: branch.clone(),
                reason,
            }),
        }
    }
    Ok(workspace)
}

fn read_components_at_branch(repo: &GitRepo, branch: &str) -> Result<Vec<DepPin>, String> {
    let commit = resolve_branch(repo, branch)?;
    let blobs = repo
        .read_paths_at_commit(&commit, |p| p == COMPONENTS_MK_PATH)
        .map_err(|e| e.to_string())?;
    let blob = blobs
        .into_iter()
        .next()
        .ok_or_else(|| format!("{COMPONENTS_MK_PATH} absent at {branch}"))?;
    let text = str::from_utf8(&blob.bytes).map_err(|e| e.to_string())?;
    Ok(parse_components_mk(text))
}

pub fn build_toml(snapshot_dir: &Path, w: &InferredWorkspace) -> String {
    let mut out = String::new();
    out.push_str("config_version = 1\n\n");
    out.push_str("[defaults]\n");
    out.push_str(&format!(
        "snapshot_dir = {:?}\n",
        snapshot_dir.display().to_string()
    ));
    out.push_str("fallback_branch = \"main\"\n");
    out.push_str("scan_paths = [\"src/**/*.erl\", \"src/**/*.ex\", \"include/**/*.hrl\"]\n");
    if w.projects.is_empty() {
        out.push_str("\n# Add one [[project]] per dependency you want to track. Example:\n");
        out.push_str("# [[project]]\n");
        out.push_str("# name    = \"lib_a\"\n");
        out.push_str("# git_url = \"/abs/path/to/lib_a.git\"\n");
    } else {
        out.push_str("\n[[project]]\n");
        out.push_str("name      = \"rabbitmq-server\"\n");
        out.push_str("kind      = \"self\"\n");
        out.push_str("layout    = \"multi_app\"\n");
        out.push_str("family    = \"rabbitmq\"\n");
        out.push_str("app_roots = [\"deps\"]\n");
        for p in w.projects.values() {
            out.push_str("\n[[project]]\n");
            out.push_str(&format!("name       = \"{}\"\n", p.name));
            out.push_str("language   = \"erlang\"\n");
            out.push_str("git_url    = \"TODO: set to a clone URL or local path\"\n");
            out.push_str(&format!("tag_prefix = \"{}\"\n", p.tag_prefix));
        }
    }
    let mut seen_series: BTreeSet<&str> = BTreeSet::new();
    for s in &w.series {
        if !seen_series.insert(s.name.as_str()) {
            continue;
        }
        out.push_str("\n[[series]]\n");
        out.push_str(&format!("name = \"{}\"  # from {}\n", s.name, s.branch));
        out.push_str("pins = [\n");
        let widest = s
            .pins
            .iter()
            .map(|p| p.project.as_str().len())
            .max()
            .unwrap_or(0);
        let rabbitmq_pin = format!(
            "    {{ project = \"rabbitmq-server\",{:gap$} branch = \"{}\" }},\n",
            "",
            s.branch,
            gap = widest.saturating_sub("rabbitmq-server".len()),
        );
        out.push_str(&rabbitmq_pin);
        for pin in &s.pins {
            let project_str = pin.project.as_str();
            out.push_str(&format!(
                "    {{ project = \"{}\",{:gap$} tag = \"{}\" }},\n",
                project_str,
                "",
                pin.tag,
                gap = widest - project_str.len(),
            ));
        }
        out.push_str("]\n");
    }
    out
}

fn render_text(w: &mut dyn Write, payload: &InitPayload) -> CliResult<()> {
    writeln!(w, "wrote {}", payload.wrote.display())?;
    writeln!(w, "snapshot_dir: {}", payload.snapshot_dir.display())?;
    if !payload.projects.is_empty() {
        writeln!(
            w,
            "projects: {} (each has `git_url = TODO` to fill in)",
            payload.projects.len()
        )?;
    }
    if !payload.series.is_empty() {
        writeln!(w, "series: {}", payload.series.join(", "))?;
    }
    if !payload.skipped_branches.is_empty() {
        writeln!(w, "skipped {} branch(es):", payload.skipped_branches.len())?;
        for s in &payload.skipped_branches {
            writeln!(w, "  {}: {}", s.branch, s.reason)?;
        }
    }
    writeln!(w, "next: `backhopper doctor` to see coverage gaps")?;
    Ok(())
}
