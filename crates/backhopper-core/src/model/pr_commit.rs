// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Inner PR-branch commits enumerated under a 2-parent merge.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::model::names::CommitSha;

/// One commit on the PR-branch side of a 2-parent merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PrCommit {
    pub sha: CommitSha,
    pub subject: String,
    #[cfg_attr(feature = "schemars", schemars(with = "String"))]
    #[serde(with = "time::serde::rfc3339")]
    pub author_date: OffsetDateTime,
    pub kind: PrCommitKind,
}

/// Classification of a PR-branch commit's role.
///
/// `#[non_exhaustive]` so future, richer taxonomies (test-only,
/// docs-only, dependency-bump, etc.) can add variants without a
/// breaking change.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PrCommitKind {
    /// Default: substantive change worth reviewing.
    Substantive,
    /// Subject matched the conflict-resolution catalog AND the file
    /// set was a strict subset of prior PR-branch commits.
    ConflictResolution,
    /// Subject indicated the commit applied code-review feedback.
    ReviewFeedback,
    /// `git --fixup` auto-generated or the manual `fixup:` convention.
    Fixup,
    /// Subject was `WIP` or bare `Cleanup`.
    WipOrCleanup,
}

impl PrCommitKind {
    /// True when the kind is anything other than `Substantive`. Useful
    /// shorthand for "skip this row by default."
    #[must_use]
    pub fn is_likely_bookkeeping(self) -> bool {
        !matches!(self, Self::Substantive)
    }
}
