// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Public wire-format types for `check batch`. Promoted from the
//! previously-private types in `backhopper-cli::commands::check`.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::model::names::{CommitSha, ProjectName, RelativePath, SeriesName, TagName};
use crate::model::pin::Pin;
use crate::model::pr_commit::PrCommit;
use crate::model::verdict::{Diagnostics, PatchFacts, SeriesVerdict};

/// Top-level payload of a `check batch` envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BatchPayload {
    pub queried_against: Vec<BatchQuery>,
    pub results: Vec<BatchResult>,
}

/// One series queried by the batch. Holds the pin set used during
/// evaluation so consumers can correlate per-row results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BatchQuery {
    pub series: SeriesName,
    pub pins: Vec<PinPayload>,
}

/// On-wire pin shape used inside `BatchQuery`. Distinct from
/// `crate::model::pin::Pin` because the wire form is a `(project, tag)`
/// projection without the resolution machinery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PinPayload {
    pub project: ProjectName,
    pub tag: TagName,
}

impl From<&Pin> for PinPayload {
    fn from(pin: &Pin) -> Self {
        Self {
            project: pin.project.clone(),
            tag: pin.tag.clone(),
        }
    }
}

/// One row in the batch result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BatchResult {
    pub commit: CommitSha,
    pub series: SeriesName,
    pub verdict: SeriesVerdict,

    #[serde(default, skip_serializing_if = "Diagnostics::is_empty")]
    pub diagnostics: Diagnostics,

    #[serde(default, skip_serializing_if = "PatchFacts::is_empty")]
    pub patch_facts: PatchFacts,

    /// Paths the analysed patch touched, in diff-encounter order.
    /// Always emitted; an empty vec means the patch touched zero
    /// files.
    #[serde(default)]
    pub touched_paths: Vec<RelativePath>,

    /// Inner PR-branch commits for 2-parent merge SHAs. `None` for
    /// non-2-parent merges and non-merge SHAs.
    ///
    /// Do NOT add `skip_serializing_if = "Option::is_none"`. The
    /// `None` vs `Some(vec![])` distinction is wire-load-bearing.
    #[serde(default)]
    pub pr_commits: Option<Vec<PrCommit>>,

    /// Number of parents of the evaluated commit: 1 for plain
    /// commits, 2 for PR merges, 3+ for octopus merges. `None` only
    /// when the envelope was produced by a pre-v7 binary; v7
    /// producers always emit it.
    #[serde(default)]
    pub parent_count: Option<NonZeroU32>,
}
