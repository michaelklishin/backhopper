// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper rev resolve <COMMIT_SHA>`: expand a SHA prefix to its
//! full 40-character form using the shared resolver.

use crate::outcome::CommandOutcome;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use backhopper_core::model::names::{CommitSha, CommitShaPrefix};
use backhopper_git::{GitError, GitRepo, ObjectKind};

use crate::cli::{GlobalArgs, RevCmd};
use crate::commands::sha_prefix::resolve_with_kind;
use crate::commands::summary::SummaryFormatter;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, emit_jsonl, render_with_exit};

// envelope renders first; the typed CliError below sets the non-zero exit.
const ENVELOPE_ONLY_EXIT: CommandOutcome = CommandOutcome::Success;

#[derive(Debug, Serialize)]
struct ResolvePayload {
    input: String,
    resolved: CommitSha,
    object_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
}

#[derive(Debug, Serialize)]
struct AmbiguousPayload {
    input: String,
    candidates: Vec<String>,
    truncated_at: u32,
}

#[derive(Debug, Serialize)]
struct NotACommitPayload {
    input: String,
    object_kind: String,
    resolved: String,
}

#[derive(Debug, Serialize)]
struct NotFoundPayload {
    input: String,
}

pub fn handle(args: &GlobalArgs, cmd: RevCmd) -> CliResult<CommandOutcome> {
    match cmd {
        RevCmd::Resolve {
            repo_dir_path,
            subject,
            commit,
        } => run_resolve(args, &repo_dir_path, subject, &commit),
    }
}

fn run_resolve(
    args: &GlobalArgs,
    repo_dir_path: &Path,
    want_subject: bool,
    prefix: &CommitShaPrefix,
) -> CliResult<CommandOutcome> {
    match resolve_with_kind(repo_dir_path, prefix) {
        Ok(resolved) => render_resolved(
            args,
            prefix,
            &resolved.commit,
            resolved.kind,
            repo_dir_path,
            want_subject,
        ),
        Err(CliError::Git(GitError::AmbiguousSha {
            prefix: p,
            candidates,
            truncated_at,
        })) => render_ambiguous(args, &p, &candidates, truncated_at),
        Err(CliError::Git(GitError::NotACommit {
            prefix: p,
            kind,
            resolved,
        })) => render_not_a_commit(args, &p, &kind, &resolved),
        Err(CliError::Git(GitError::CommitNotFound(p))) => render_not_found(args, &p),
        Err(other) => Err(other),
    }
}

fn render_resolved(
    args: &GlobalArgs,
    prefix: &CommitShaPrefix,
    commit: &CommitSha,
    kind: ObjectKind,
    repo_dir_path: &Path,
    want_subject: bool,
) -> CliResult<CommandOutcome> {
    let subject = if want_subject {
        GitRepo::open(repo_dir_path.to_path_buf())
            .ok()
            .and_then(|r| r.commit_subject(commit).ok())
    } else {
        None
    };
    let payload = ResolvePayload {
        input: prefix.to_string(),
        resolved: commit.clone(),
        object_kind: kind.to_string(),
        subject,
    };
    if let Some(fmt) = SummaryFormatter::from_cli(args.formatter) {
        let mut stdout = io::stdout().lock();
        match fmt {
            // the text-summary body is just the resolved SHA
            SummaryFormatter::Text => {
                writeln!(stdout, "{commit}").map_err(|e| CliError::OutputError(e.to_string()))?;
            }
            SummaryFormatter::Jsonl => emit_jsonl(&mut stdout, &[payload])?,
        }
        return Ok(CommandOutcome::Success);
    }
    let ctx = OutputContext::new(args.formatter, "rev resolve");
    render_with_exit(&ctx, &payload, CommandOutcome::Success, |w| {
        writeln!(w, "input:       {}", payload.input)?;
        writeln!(w, "resolved:    {}", payload.resolved)?;
        writeln!(w, "object_kind: {}", payload.object_kind)?;
        if let Some(s) = &payload.subject {
            writeln!(w, "subject:     {s}")?;
        }
        Ok(())
    })
}

/// Emit the failure envelope (exit 0), then carry the non-zero exit and
/// the user-facing message through the typed `CliError`.
fn render_failure<P: Serialize>(
    args: &GlobalArgs,
    payload: &P,
    message: String,
) -> CliResult<CommandOutcome> {
    let ctx = OutputContext::new(args.formatter, "rev resolve");
    render_with_exit(&ctx, payload, ENVELOPE_ONLY_EXIT, |_| Ok(()))?;
    Err(CliError::InvalidInput(message))
}

fn render_ambiguous(
    args: &GlobalArgs,
    prefix: &str,
    candidates: &[String],
    truncated_at: u32,
) -> CliResult<CommandOutcome> {
    let payload = AmbiguousPayload {
        input: prefix.to_owned(),
        candidates: candidates.to_vec(),
        truncated_at,
    };
    render_failure(
        args,
        &payload,
        format!(
            "prefix {prefix:?} matched {truncated_at} object(s); extend the prefix by one or more characters and try again"
        ),
    )
}

fn render_not_a_commit(
    args: &GlobalArgs,
    prefix: &str,
    kind: &str,
    resolved: &str,
) -> CliResult<CommandOutcome> {
    let payload = NotACommitPayload {
        input: prefix.to_owned(),
        object_kind: kind.to_owned(),
        resolved: resolved.to_owned(),
    };
    render_failure(
        args,
        &payload,
        format!("prefix {prefix:?} resolved to a {kind} (object {resolved}), not a commit"),
    )
}

fn render_not_found(args: &GlobalArgs, prefix: &str) -> CliResult<CommandOutcome> {
    let payload = NotFoundPayload {
        input: prefix.to_owned(),
    };
    render_failure(
        args,
        &payload,
        format!("commit prefix {prefix:?} not found in repository"),
    )
}
