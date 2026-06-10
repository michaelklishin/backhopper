// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! CLI error type. Maps to BSD `sysexits` via `bel7_cli::ExitCodeProvider`.

use std::io;
use std::path::PathBuf;

use bel7_cli::{ExitCode, ExitCodeProvider, codes};
use thiserror::Error;

use backhopper_core::Error as CoreError;
use backhopper_core::errors::{ConfigError, StoreError};
use backhopper_core::model::names::SeriesName;
use backhopper_core::model::pin::Pin;
use backhopper_git::{GitError, PatchInputError};

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Core(#[from] CoreError),

    #[error("git error: {0}")]
    Git(#[from] GitError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("config not found; tried (in order): {}", tried_display(.tried))]
    ConfigNotFound { tried: Vec<PathBuf> },

    #[error("output error: {0}")]
    OutputError(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("snapshots missing for {} pin(s)", missing.len())]
    MissingSnapshots {
        missing: Vec<Pin>,
        remediation: String,
    },

    #[error("snapshot directory {} resolves to {}, which escapes the workspace root {}", configured.display(), resolved.display(), root.display())]
    SnapshotDirEscape {
        configured: PathBuf,
        resolved: PathBuf,
        root: PathBuf,
    },

    #[error("series {series} has no self-pin to derive the target branch from")]
    SeriesHasNoSelfPin { series: SeriesName },

    #[error(
        "no release tag{} is reachable from {target_branch}; cannot derive a default --since",
        pattern.as_deref().map(|p| format!(" matching {p:?}")).unwrap_or_default()
    )]
    NoReachableSinceTag {
        target_branch: String,
        pattern: Option<String>,
        shallow: bool,
    },

    #[error("{0}")]
    Other(String),
}

impl ExitCodeProvider for CliError {
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Core(e) => map_core(e),
            Self::Git(_) => codes::IO_ERR,
            Self::Io(_) => codes::IO_ERR,
            Self::ConfigNotFound { .. } => codes::NO_INPUT,
            Self::OutputError(_) => codes::IO_ERR,
            Self::InvalidInput(_) => codes::USAGE,
            Self::MissingSnapshots { .. } => codes::NO_INPUT,
            Self::SnapshotDirEscape { .. } => codes::USAGE,
            Self::SeriesHasNoSelfPin { .. } => codes::USAGE,
            Self::NoReachableSinceTag { .. } => codes::USAGE,
            Self::Other(_) => codes::SOFTWARE,
        }
    }
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        <Self as ExitCodeProvider>::exit_code(self)
    }

    /// One-line next-action hint printed after the primary error.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::ConfigNotFound { .. }
            | Self::Core(CoreError::Config(ConfigError::NotFound(_))) => {
                Some(CONFIG_NOT_FOUND_HINT.into())
            }
            Self::Core(CoreError::Store(StoreError::SnapshotNotFound { project, tag })) => {
                Some(format!(
                    "run `backhopper snapshots generate --project {project} --since {tag}` then retry"
                ))
            }
            Self::Core(CoreError::Store(StoreError::RootMissing(p))) => Some(format!(
                "create the snapshot directory or pass `--snapshot-dir-path <PATH>`; missing: {}",
                p.display()
            )),
            Self::Core(CoreError::Store(StoreError::PathEscape(_)))
            | Self::SnapshotDirEscape { .. } => Some(SNAPSHOT_DIR_ESCAPE_HINT.into()),
            Self::Core(CoreError::Config(ConfigError::UnknownProject(_)))
            | Self::Core(CoreError::Config(ConfigError::UnknownSeries(_))) => Some(
                "list configured names with `backhopper projects list` or `backhopper series list`"
                    .into(),
            ),
            Self::Core(CoreError::Config(ConfigError::PinPatternNoMatch { project, .. })) => {
                Some(format!(
                    "run `backhopper snapshots generate --project {project}` so the pattern has tags to resolve against"
                ))
            }
            Self::Git(g) => git_hint(g),
            Self::Core(CoreError::Patch(_)) => Some(
                "ensure the input is a unified diff (e.g. `git format-patch -1 --stdout`)".into(),
            ),
            Self::MissingSnapshots { remediation, .. } => Some(remediation.clone()),
            Self::SeriesHasNoSelfPin { .. } => Some(
                "pass `--target-branch <REF>`, or add a self-pin (`branch = \"...\"`) to the series"
                    .into(),
            ),
            Self::NoReachableSinceTag { shallow, .. } => Some(if *shallow {
                "pass `--since <SHA|TAG>` to bound the walk; this clone is shallow, so `git fetch --unshallow` may also restore tag reachability".into()
            } else {
                "pass `--since <SHA|TAG>` to bound the walk explicitly".into()
            }),
            _ => None,
        }
    }

    /// Multi-line detail block printed between the primary error and the hint.
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::MissingSnapshots { missing, .. } => {
                let mut out = String::from("missing:\n");
                for pin in missing {
                    out.push_str(&format!("  {} @ {}\n", pin.project, pin.tag));
                }
                out.pop();
                Some(out)
            }
            _ => None,
        }
    }
}

impl From<PatchInputError> for CliError {
    fn from(e: PatchInputError) -> Self {
        match e {
            PatchInputError::Git(g) => Self::Git(g),
            other => Self::InvalidInput(other.to_string()),
        }
    }
}

const CONFIG_NOT_FOUND_HINT: &str = "run `backhopper init` to create a starter backhopper.toml in the current directory, \
     or pass `--config-file-path <PATH>`";

const SNAPSHOT_DIR_ESCAPE_HINT: &str = "set `[defaults] snapshot_dir` to an absolute path in your backhopper.toml, \
     or pass `--snapshot-dir-path <ABS>` on the command line";

fn git_hint(e: &GitError) -> Option<String> {
    match e {
        GitError::CommitNotFound(_) => Some(
            "if this is a missing commit, try `git fetch` in the target repository \
                  (or `git fetch --unshallow` if the clone is shallow)"
                .into(),
        ),
        GitError::TagNotFound(_) => Some(
            "if the tag exists upstream, try `git fetch --tags` in the target repository".into(),
        ),
        GitError::OpenFailed(_) => Some(
            "check that `git_url` points at a valid git directory (bare or working tree)".into(),
        ),
        GitError::NotAGitRepository(_) => Some(
            "pass `--repo-dir-path <PATH>` pointing at a git directory (bare or working tree)"
                .into(),
        ),
        GitError::AmbiguousSha { .. } => {
            Some("extend the prefix by one or more characters and try again".into())
        }
        GitError::NotACommit { .. } => Some(
            "the prefix resolved to a non-commit object; check that the SHA names a commit".into(),
        ),
        GitError::PathNotPresent { .. } => Some(
            "the path is absent at that commit; verify the file exists at the pinned tag".into(),
        ),
        GitError::Gix(_) | GitError::Io(_) => None,
    }
}

fn tried_display(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn map_core(e: &CoreError) -> ExitCode {
    match e {
        CoreError::Snapshot(_) => codes::DATA_ERR,
        CoreError::Store(s) => match s {
            StoreError::SnapshotNotFound { .. } | StoreError::RootMissing(_) => codes::NO_INPUT,
            _ => codes::IO_ERR,
        },
        CoreError::Config(c) => match c {
            ConfigError::NotFound(_) => codes::NO_INPUT,
            _ => codes::DATA_ERR,
        },
        CoreError::Io(_) => codes::IO_ERR,
        CoreError::Patch(_) => codes::DATA_ERR,
        CoreError::Name(_) => codes::USAGE,
    }
}
