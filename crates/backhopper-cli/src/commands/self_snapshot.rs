// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Self-snapshot generation: snapshot the working repo at a `git_ref`
//! (branch, tag, or SHA), keyed by the resolved commit SHA in the
//! existing `SnapshotStore`. Re-resolution is free until HEAD of the
//! branch moves, because the synthetic tag is the resolved SHA itself.

use std::path::Path;

use backhopper_core::Error as CoreError;
use backhopper_core::config::{Config, Project};
use backhopper_core::model::names::{CommitSha, TagName};
use backhopper_core::model::pin::{Pin, PinSpec};
use backhopper_core::store::{Mutable, ReadOnly, SnapshotStore};
use backhopper_git::GitRepo;

use crate::cli::GlobalArgs;
use crate::commands::context::open_store_mut;
use crate::commands::snapshots::build_snapshot_at_commit;
use crate::errors::{CliError, CliResult};

/// Pick the repo path a self-pin should resolve against. The per-pin
/// `repo_dir_path` on `PinSpec::SelfRef` wins; the CLI `--repo-dir-path`
/// is the fallback. Both unset is the user error this returns.
pub fn effective_self_repo<'a>(
    spec: &'a PinSpec,
    cli_fallback: Option<&'a Path>,
) -> CliResult<&'a Path> {
    if let Some(p) = spec.self_repo_override() {
        return Ok(p);
    }
    cli_fallback.ok_or_else(|| {
        CliError::InvalidInput(format!(
            "self-pin {} has no `repo_dir_path` in the config and no `--repo-dir-path` was given",
            spec.project()
        ))
    })
}

/// Resolve a self-pin's `git_ref` to a concrete `Pin` whose tag is the
/// resolved commit SHA. The caller writes the snapshot to the store under
/// that tag via `ensure_self_snapshot_present` before evaluation.
pub fn resolve_self_pin(cli_fallback: Option<&Path>, spec: &PinSpec) -> CliResult<Pin> {
    let (project, git_ref) = match spec {
        PinSpec::SelfRef {
            project, git_ref, ..
        } => (project, git_ref),
        _ => unreachable!("resolve_self_pin called on non-self pin"),
    };
    let self_repo = effective_self_repo(spec, cli_fallback)?;
    let repo = GitRepo::open(self_repo.to_path_buf()).map_err(CliError::Git)?;
    let commit = repo.resolve_rev(git_ref.as_str()).map_err(CliError::Git)?;
    let tag = TagName::new(commit.as_str()).map_err(|e| {
        CliError::Other(format!(
            "self pin {project}@{git_ref} resolved to invalid sha {}: {e}",
            commit.as_str()
        ))
    })?;
    Ok(Pin::new(project.clone(), tag))
}

/// Materialize the self-pin's snapshot into the store if absent. Cached
/// by `(project, sha-as-tag)`: subsequent runs hit the existing file.
#[allow(clippy::disallowed_methods)]
pub fn ensure_self_snapshot_present(
    args: &GlobalArgs,
    cfg: &Config,
    store: &SnapshotStore<ReadOnly>,
    project: &Project,
    spec: &PinSpec,
    cli_fallback: Option<&Path>,
    pin: &Pin,
) -> CliResult<()> {
    if store.has(&pin.project, &pin.tag) {
        return Ok(());
    }
    let self_repo = effective_self_repo(spec, cli_fallback)?;
    let repo = GitRepo::open(self_repo.to_path_buf()).map_err(CliError::Git)?;
    let commit = CommitSha::new(pin.tag.as_str()).map_err(|e| CliError::Other(e.to_string()))?;
    let snapshot = build_snapshot_at_commit(project, &repo, &commit, &pin.tag)?;
    let mut_store: SnapshotStore<Mutable> = open_store_mut(args, cfg)?;
    mut_store
        .write(&snapshot)
        .map_err(|e| CliError::Core(CoreError::Store(e)))?;
    Ok(())
}
