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
use crate::model::names::{GitRef, ProjectName, SeriesName, TagGlob, TagName};
use crate::store::{SnapshotStore, StoreMode};
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
        // version_cmp orders descending: newer is Less, older is Greater
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
    /// is split across several physical git directories (one per maintained
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

    /// `SelfRef` pins cannot be resolved here: the working repo is outside
    /// the snapshot store. The CLI resolves them via the `--repo-dir-path`
    /// before calling into the evaluation pipeline.
    pub fn resolve<M: StoreMode>(&self, store: &SnapshotStore<M>) -> Result<Pin, ConfigError> {
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

    /// `Some` with the self-ref payload for a `SelfRef` spec, `None` for
    /// `Literal` and `Pattern`, which have no repo to resolve against.
    pub fn as_self_pin(&self) -> Option<SelfPin<'_>> {
        match self {
            Self::SelfRef {
                project,
                git_ref,
                repo_dir_path,
            } => Some(SelfPin {
                project,
                git_ref,
                repo_dir_path: repo_dir_path.as_deref(),
            }),
            Self::Literal { .. } | Self::Pattern { .. } => None,
        }
    }
}

/// The self-ref half of a `PinSpec`, handed to the one function that
/// resolves it so a caller cannot pass a literal or pattern spec by mistake.
#[derive(Debug, Clone, Copy)]
pub struct SelfPin<'a> {
    pub project: &'a ProjectName,
    pub git_ref: &'a GitRef,
    /// `Some` when the pin overrides the CLI `--repo-dir-path` fallback.
    pub repo_dir_path: Option<&'a Path>,
}

/// Either a named series or a single `(project, tag)` pin: the "where" a
/// check runs, addressed by `--series` or by `--project` and `--tag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinSelector {
    Series(SeriesName),
    Pin { project: ProjectName, tag: TagName },
}

impl PinSelector {
    pub fn series(name: impl Into<SeriesName>) -> Self {
        Self::Series(name.into())
    }

    /// The series a `Series` selector names; `None` for a single pin.
    #[must_use]
    pub fn series_name(&self) -> Option<&SeriesName> {
        match self {
            Self::Series(name) => Some(name),
            Self::Pin { .. } => None,
        }
    }

    pub fn pin(project: impl Into<ProjectName>, tag: impl Into<TagName>) -> Self {
        Self::Pin {
            project: project.into(),
            tag: tag.into(),
        }
    }
}

impl From<SeriesName> for PinSelector {
    fn from(s: SeriesName) -> Self {
        Self::Series(s)
    }
}

impl From<(ProjectName, TagName)> for PinSelector {
    fn from((project, tag): (ProjectName, TagName)) -> Self {
        Self::Pin { project, tag }
    }
}

/// Resolve a slice of `PinSpec`s into concrete `Pin`s by consulting `store`.
/// Returns at the first resolution failure.
pub fn resolve_all<M: StoreMode>(
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
