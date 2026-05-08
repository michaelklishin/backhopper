use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use backhopper_core::compat::patch::{Language, Patch, PinFiles};
use backhopper_core::config::Config;
use backhopper_core::git::GitRepo;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::SeriesVerdict;

use crate::cli::{CompatibilityCmd, GlobalArgs};
use crate::commands::context::{load_config, open_store_read};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

#[derive(Debug, Serialize)]
struct CompatPayload {
    queried_against: QueriedAgainst,
    results: SeriesVerdict,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum QueriedAgainst {
    Pin { project: String, tag: String },
    Series { name: String, pins: Vec<PinPayload> },
}

#[derive(Debug, Serialize)]
struct PinPayload {
    project: String,
    tag: String,
}

pub fn handle(args: &GlobalArgs, cmd: CompatibilityCmd) -> CliResult<i32> {
    let cfg = load_config(args)?;
    match cmd {
        CompatibilityCmd::Patch {
            project,
            tag,
            series,
            explain: _,
            patch_file,
        } => {
            let bytes = read_patch_input(patch_file)?;
            run_compat_patch(args, &cfg, &bytes, project, tag, series)
        }
        CompatibilityCmd::Commit {
            project,
            tag,
            series,
            repo,
            commit,
        } => {
            let bytes = commit_patch_bytes(&repo, &commit)?;
            run_compat_patch(args, &cfg, &bytes, project, tag, series)
        }
        CompatibilityCmd::Range {
            project,
            tag,
            series,
            repo,
            range,
            merge_commit,
        } => {
            let bytes = range_patch_bytes(&repo, range.as_deref(), merge_commit.as_deref())?;
            run_compat_patch(args, &cfg, &bytes, project, tag, series)
        }
    }
}

fn build_pin_files(
    cfg: &Config,
    pin: &Pin,
    touched_paths: &[std::path::PathBuf],
) -> CliResult<PinFiles> {
    if touched_paths.is_empty() {
        return Ok(PinFiles::new());
    }
    let project = cfg
        .project(&pin.project)
        .map_err(|e| CliError::Core(e.into()))?;
    let repo = GitRepo::open(project.git_url.clone()).map_err(|e| CliError::Core(e.into()))?;
    let commit = repo
        .resolve_tag(&pin.tag)
        .map_err(|e| CliError::Core(e.into()))?;
    let needed: std::collections::BTreeSet<String> = touched_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let blobs = repo
        .read_paths_at_commit(&commit, |p| needed.contains(p))
        .map_err(|e| CliError::Core(e.into()))?;
    let mut files = PinFiles::new();
    let present: std::collections::BTreeMap<String, Vec<u8>> = blobs
        .into_iter()
        .map(|b| (b.path.to_string_lossy().into_owned(), b.bytes))
        .collect();
    for path in touched_paths {
        let key = path.to_string_lossy().into_owned();
        let contents = present.get(&key).cloned();
        files = files.with(path.clone(), contents);
    }
    Ok(files)
}

fn read_patch_input(file: Option<PathBuf>) -> CliResult<Vec<u8>> {
    match file {
        Some(p) => Ok(std::fs::read(&p)?),
        None => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            if buf.is_empty() {
                return Err(CliError::InvalidInput(
                    "no patch on stdin and no --patch-file given".into(),
                ));
            }
            Ok(buf)
        }
    }
}

fn commit_patch_bytes(repo: &Path, commit: &str) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let to = CommitSha::new(commit.to_owned())
        .map_err(|e| CliError::Core(backhopper_core::Error::Name(e)))?;
    let from = g
        .parent_commit(&to)
        .map_err(|e| CliError::Core(e.into()))?
        .ok_or_else(|| CliError::InvalidInput(format!("commit {} has no parent", commit)))?;
    let text = g
        .diff_commits_unified(&from, &to, |p| {
            p.ends_with(".erl") || p.ends_with(".hrl") || p.ends_with(".ex") || p.ends_with(".exs")
        })
        .map_err(|e| CliError::Core(e.into()))?;
    Ok(text.into_bytes())
}

fn range_patch_bytes(repo: &Path, range: Option<&str>, merge: Option<&str>) -> CliResult<Vec<u8>> {
    let g = GitRepo::open(repo.to_path_buf()).map_err(|e| CliError::Core(e.into()))?;
    let (from, to) = match (range, merge) {
        (Some(r), None) => {
            let (a, b) = r
                .split_once("..")
                .ok_or_else(|| CliError::InvalidInput(format!("invalid range {:?}", r)))?;
            (
                CommitSha::new(a.to_owned())
                    .map_err(|e| CliError::Core(backhopper_core::Error::Name(e)))?,
                CommitSha::new(b.to_owned())
                    .map_err(|e| CliError::Core(backhopper_core::Error::Name(e)))?,
            )
        }
        (None, Some(merge_sha)) => {
            let merge = CommitSha::new(merge_sha.to_owned())
                .map_err(|e| CliError::Core(backhopper_core::Error::Name(e)))?;
            let parents = g.parents(&merge).map_err(|e| CliError::Core(e.into()))?;
            if parents.len() < 2 {
                return Err(CliError::InvalidInput(format!(
                    "{} is not a merge commit (parents: {})",
                    merge_sha,
                    parents.len()
                )));
            }
            (parents[0].clone(), merge)
        }
        _ => {
            return Err(CliError::InvalidInput(
                "specify either --range BASE..HEAD or --merge-commit SHA".into(),
            ));
        }
    };
    let text = g
        .diff_commits_unified(&from, &to, |p| {
            p.ends_with(".erl") || p.ends_with(".hrl") || p.ends_with(".ex") || p.ends_with(".exs")
        })
        .map_err(|e| CliError::Core(e.into()))?;
    Ok(text.into_bytes())
}

fn run_compat_patch(
    args: &GlobalArgs,
    cfg: &Config,
    bytes: &[u8],
    project: Option<ProjectName>,
    tag: Option<TagName>,
    series: Option<SeriesName>,
) -> CliResult<i32> {
    let store = open_store_read(args, cfg)?;
    let pins: Vec<Pin> = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => vec![Pin::new(p.clone(), t.clone())],
        (None, None, Some(s)) => {
            let s = cfg
                .series_by_name(s)
                .map_err(|e| CliError::Core(e.into()))?;
            s.pins.clone()
        }
        _ => {
            return Err(CliError::InvalidInput(
                "specify either --project + --tag, or --series".into(),
            ));
        }
    };
    let patch =
        Patch::parse(bytes).map_err(|e| CliError::Core(backhopper_core::Error::Patch(e)))?;
    let touched_paths: Vec<std::path::PathBuf> = patch
        .files
        .iter()
        .filter_map(|f| f.new_path.clone().or_else(|| f.old_path.clone()))
        .collect();
    let analyzed = patch.analyze(Language::Erlang);
    let mut snapshots_with_files = Vec::with_capacity(pins.len());
    for pin in &pins {
        let snap = store
            .read(&pin.project, &pin.tag)
            .map_err(|e| CliError::Core(e.into()))?;
        let files = build_pin_files(cfg, pin, &touched_paths)?;
        snapshots_with_files.push((pin.clone(), snap, files));
    }
    let series_verdict = analyzed.against_series_with_files(&snapshots_with_files);
    let queried = match (&project, &tag, &series) {
        (Some(p), Some(t), None) => QueriedAgainst::Pin {
            project: p.to_string(),
            tag: t.to_string(),
        },
        (None, None, Some(s)) => QueriedAgainst::Series {
            name: s.to_string(),
            pins: pins
                .iter()
                .map(|p| PinPayload {
                    project: p.project.to_string(),
                    tag: p.tag.to_string(),
                })
                .collect(),
        },
        _ => unreachable!(),
    };
    let payload = CompatPayload {
        queried_against: queried,
        results: series_verdict.clone(),
    };
    let ctx = OutputContext::new(args.formatter, "compatibility patch");
    let exit = series_verdict.worst_exit_code();
    render_with_exit(&ctx, &payload, exit, |w| {
        writeln!(
            w,
            "compatible: {}, requires_adaptation: {}, incompatible: {}",
            series_verdict.summary.compatible,
            series_verdict.summary.requires_adaptation,
            series_verdict.summary.incompatible,
        )?;
        for r in &series_verdict.results {
            writeln!(w, "  {}: {:?}", r.pin, r.verdict)?;
        }
        Ok(())
    })
}
