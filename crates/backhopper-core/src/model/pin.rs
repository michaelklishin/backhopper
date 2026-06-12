// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Pins as a two-stage type: `PinSpec` at config load, `Pin` after resolution.
//!
//! A `PinSpec` either names a concrete tag (`Literal`) or describes a set of
//! candidate tags via a glob plus a selection rule (`Pattern`). Resolution
//! against a snapshot store turns either form into a `Pin`, which the rest of
//! the compatibility pipeline treats as the only pin type.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::errors::ConfigError;
use crate::model::names::{GitRef, ProjectName, TagGlob, TagName};
use crate::store::SnapshotStore;
use crate::versions::version_cmp;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Pin {
    pub project: ProjectName,
    pub tag: TagName,
}

impl Pin {
    pub fn new(project: ProjectName, tag: TagName) -> Self {
        Self { project, tag }
    }
}

impl fmt::Display for Pin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.project, self.tag)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PinSelect {
    Latest,
    Oldest,
}

impl PinSelect {
    /// Pick the tag matching the selection from an iterator of candidates.
    pub fn pick<'a, I>(self, tags: I) -> Option<&'a TagName>
    where
        I: IntoIterator<Item = &'a TagName>,
    {
        let iter = tags.into_iter();
        // `version_cmp` orders descending: newer is `Less`, older is `Greater`.
        match self {
            Self::Latest => iter.min_by(|a, b| version_cmp(a.as_str(), b.as_str())),
            Self::Oldest => iter.max_by(|a, b| version_cmp(a.as_str(), b.as_str())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PinSpec {
    Literal {
        project: ProjectName,
        tag: TagName,
    },
    Pattern {
        project: ProjectName,
        #[serde(rename = "tag_pattern")]
        pattern: TagGlob,
        select: PinSelect,
    },
    /// Self-project pin. `git_ref` is a branch, tag, or SHA resolved in the
    /// working repo at evaluation time. `repo_dir_path`, when set, overrides
    /// the CLI `--repo-dir-path` fallback: needed when one logical project
    /// lives in several physical git directories (one per maintained
    /// branch).
    SelfRef {
        project: ProjectName,
        git_ref: GitRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo_dir_path: Option<PathBuf>,
    },
}

impl PinSpec {
    pub fn literal(project: ProjectName, tag: TagName) -> Self {
        Self::Literal { project, tag }
    }

    pub fn pattern(project: ProjectName, pattern: TagGlob, select: PinSelect) -> Self {
        Self::Pattern {
            project,
            pattern,
            select,
        }
    }

    pub fn project(&self) -> &ProjectName {
        match self {
            Self::Literal { project, .. }
            | Self::Pattern { project, .. }
            | Self::SelfRef { project, .. } => project,
        }
    }

    pub fn is_self(&self) -> bool {
        matches!(self, Self::SelfRef { .. })
    }

    /// `SelfRef` pins cannot be resolved here: the working repo lives outside
    /// the snapshot store. The CLI resolves them via the `--repo-dir-path`
    /// before calling into the evaluation pipeline.
    pub fn resolve<M>(&self, store: &SnapshotStore<M>) -> Result<Pin, ConfigError> {
        match self {
            Self::Literal { project, tag } => Ok(Pin::new(project.clone(), tag.clone())),
            Self::Pattern {
                project,
                pattern,
                select,
            } => {
                let stored =
                    store
                        .list_tags(project)
                        .map_err(|e| ConfigError::PinPatternStore {
                            project: project.to_string(),
                            pattern: pattern.to_string(),
                            detail: e.to_string(),
                        })?;
                let chosen = select
                    .pick(stored.iter().filter(|t| pattern.matches(t)))
                    .ok_or_else(|| ConfigError::PinPatternNoMatch {
                        project: project.to_string(),
                        pattern: pattern.to_string(),
                    })?;
                Ok(Pin::new(project.clone(), chosen.clone()))
            }
            Self::SelfRef {
                project, git_ref, ..
            } => Err(ConfigError::SelfPinNeedsRepoDirPath {
                project: project.to_string(),
                git_ref: git_ref.to_string(),
            }),
        }
    }

    /// `Some(path)` when this pin carries a `repo_dir_path` override.
    /// Returns `None` for non-self pins and for self pins that should
    /// fall back to the CLI `--repo-dir-path` argument.
    pub fn self_repo_override(&self) -> Option<&Path> {
        match self {
            Self::SelfRef { repo_dir_path, .. } => repo_dir_path.as_deref(),
            _ => None,
        }
    }
}

/// Resolve a slice of `PinSpec`s into concrete `Pin`s by consulting `store`.
/// Returns at the first resolution failure.
pub fn resolve_all<M>(
    specs: &[PinSpec],
    store: &SnapshotStore<M>,
) -> Result<Vec<Pin>, ConfigError> {
    specs.iter().map(|s| s.resolve(store)).collect()
}

impl fmt::Display for PinSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Literal { project, tag } => write!(f, "{project}@{tag}"),
            Self::Pattern {
                project,
                pattern,
                select,
            } => {
                let select_label = match select {
                    PinSelect::Latest => "latest",
                    PinSelect::Oldest => "oldest",
                };
                write!(f, "{project}@{pattern} ({select_label})")
            }
            Self::SelfRef {
                project, git_ref, ..
            } => write!(f, "{project}@{git_ref} (self)"),
        }
    }
}
