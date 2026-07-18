// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper tree show <MODULE>`: one module's API surface from a
//! repo ref, extracted live and rendered in the canonical snapshot
//! format. The extraction and rendering are the snapshot pipeline's
//! own, so the text matches `snapshots show --module` for the same
//! module.

use crate::outcome::CommandOutcome;
use std::path::Path;

use serde::Serialize;

use backhopper_core::model::names::{CommitSha, GitRef, ModuleName};
use backhopper_core::snapshot::format::module_to_string;
use backhopper_core::snapshot::sort::canonicalize;
use backhopper_erlang::ErlangExtractor;
use backhopper_git::{GitRepo, build_target_tree_index};

use crate::cli::tree::EntryClass;
use crate::cli::{GlobalArgs, TreeCmd};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render_with_exit};

/// `rendered` is the canonical snapshot-format text: the same
/// serialization the snapshot pipeline writes to disk, which is the
/// module's wire-stable shape (the in-memory `Module` carries
/// non-JSON-serializable internals such as tuple-keyed clause heads).
#[derive(Debug, Serialize)]
struct TreeShowPayload {
    module: ModuleName,
    r#ref: String,
    resolved_commit: CommitSha,
    path: String,
    rendered: String,
}

pub fn handle(args: &GlobalArgs, cmd: TreeCmd) -> CliResult<CommandOutcome> {
    match cmd {
        TreeCmd::Show {
            repo_dir_path,
            r#ref,
            class,
            module,
        } => run_show(args, &repo_dir_path, &r#ref, class, &module),
    }
}

fn run_show(
    args: &GlobalArgs,
    repo_dir_path: &Path,
    r#ref: &str,
    class: Option<EntryClass>,
    module: &ModuleName,
) -> CliResult<CommandOutcome> {
    let repo = GitRepo::open(repo_dir_path.to_path_buf())
        .map_err(|e| CliError::Other(format!("{}: {e}", repo_dir_path.display())))?;
    let git_ref = GitRef::new(r#ref.to_owned()).map_err(|e| CliError::Other(e.to_string()))?;
    let index = build_target_tree_index(&repo, &git_ref)
        .map_err(|e| CliError::Other(format!("cannot resolve ref {ref}: {e}")))?;
    let Some(path) = index.module_erl_path(module) else {
        return Err(CliError::Other(format!(
            "module {module} is not on the tree at {ref} in {}",
            repo_dir_path.display()
        )));
    };
    let path_str = path.to_string_lossy().into_owned();
    let commit = index.resolved_commit().clone();
    let text = read_blob(&repo, &commit, &path_str)?
        .ok_or_else(|| CliError::Other(format!("cannot read {path_str} at {ref}")))?;
    let extractor = ErlangExtractor::new(Vec::new(), Vec::new());
    let Some(extracted) = extractor.extract_module(&text) else {
        return Err(CliError::Other(format!(
            "{path_str} at {ref} does not parse as an Erlang module"
        )));
    };
    // The same canonical ordering every snapshot goes through.
    let mut modules = vec![extracted];
    let mut headers = Vec::new();
    canonicalize(&mut modules, &mut headers);
    let rendered = module_to_string(&modules[0]).map_err(|e| CliError::Other(e.to_string()))?;
    let rendered = match class {
        Some(class) => filter_class(&rendered, class),
        None => rendered,
    };
    let payload = TreeShowPayload {
        module: module.clone(),
        r#ref: r#ref.to_owned(),
        resolved_commit: commit,
        path: path_str,
        rendered,
    };
    let ctx = OutputContext::new(args.formatter, "tree show");
    render_with_exit(&ctx, &payload, CommandOutcome::Success, |w| {
        write!(w, "{}", payload.rendered)?;
        Ok(())
    })
}

fn read_blob(repo: &GitRepo, commit: &CommitSha, path: &str) -> CliResult<Option<String>> {
    let bytes = repo
        .read_blob_at(commit, Path::new(path))
        .map_err(|e| CliError::Other(e.to_string()))?;
    Ok(bytes.and_then(|b| String::from_utf8(b).ok()))
}

/// Keep the `module` line, entry lines whose keyword matches, and any
/// deeper-indented continuation lines of a kept entry (wrapped specs,
/// record fields).
fn filter_class(rendered: &str, class: EntryClass) -> String {
    let keyword = class.keyword();
    let mut out = String::new();
    let mut keeping = false;
    for line in rendered.lines() {
        let is_entry = line.starts_with("  ") && !line.starts_with("   ");
        if is_entry {
            keeping = line[2..]
                .split_whitespace()
                .next()
                .is_some_and(|k| k == keyword);
        }
        if line.starts_with("module ") || keeping {
            out.push_str(line);
            out.push('\n');
        }
        if line.starts_with("module ") {
            keeping = false;
        }
    }
    out
}
