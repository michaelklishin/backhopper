// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Compact per-result projection emitted by `--formatter summary`
//! and `--formatter text-summary`.

use serde::{Deserialize, Serialize};

use crate::model::names::CommitSha;
use crate::model::verdict::{TouchedKinds, Verdict};

/// One row in the summary output: a compact projection of a single
/// `SeriesEvaluation` result. JSON and text formatters share the type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SummaryRow {
    pub sha: CommitSha,
    pub verdict: VerdictKind,
    pub touched: TouchedKinds,
    /// Total tracked-dep symbol references summed over every evaluated
    /// pin. Zero means the patch references no tracked-dep API; the
    /// row is safe to skip at a glance.
    pub tracked: u32,
    pub subject: String,
}

/// Kind-only projection of `Verdict`: drops the variant payloads so a
/// summary row stays single-line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Compatible,
    RequiresAdaptation,
    Incompatible,
    Inapplicable,
}

impl From<&Verdict> for VerdictKind {
    fn from(v: &Verdict) -> Self {
        match v {
            Verdict::Compatible => Self::Compatible,
            Verdict::RequiresAdaptation { .. } => Self::RequiresAdaptation,
            Verdict::Incompatible { .. } => Self::Incompatible,
            Verdict::Inapplicable { .. } => Self::Inapplicable,
        }
    }
}

impl VerdictKind {
    /// Snake-case wire form. Matches the `#[serde(rename_all)]` projection.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::RequiresAdaptation => "requires_adaptation",
            Self::Incompatible => "incompatible",
            Self::Inapplicable => "inapplicable",
        }
    }
}
