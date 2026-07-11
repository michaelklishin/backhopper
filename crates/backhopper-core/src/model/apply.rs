// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The apply axis: per-path predictions of whether a patch applies to
//! the target tree as written. Independent of every pin verdict, so an
//! `Inapplicable` row still carries its predicted conflicts.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use serde::{Deserialize, Serialize};

use crate::model::names::RelativePath;
use crate::model::verdict::ApplyConflictKind;

/// How one touched path is predicted to apply to the target tree.
/// Ranked by [`PathApplyOutcome::severity`]; payload-carrying variants
/// keep the enum out of `Ord`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PathApplyOutcome {
    /// Every hunk's preimage matches at its expected line.
    CleanExact,
    /// Applies, but at least one hunk's preimage was found at a shifted
    /// line. `line_delta` is the largest-magnitude shift.
    CleanDrifted { line_delta: i64 },
    /// The path could not be assessed; stated, never silent.
    Unassessed { reason: UnassessedReason },
    /// At least one hunk predicts a conflict. `kind` is the worst
    /// conflict kind across the path's hunks.
    Conflict { kind: ApplyConflictKind },
}

/// Why a path's apply outcome could not be assessed. The `Ord` only
/// breaks merge ties deterministically; it is not a severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UnassessedReason {
    /// The diff marks the file binary; there is no hunk text to match.
    BinaryFile,
    /// The target blob exists but is not valid UTF-8.
    TargetNotText,
}

impl PathApplyOutcome {
    /// Severity rank. `Unassessed` ranks below `Conflict`: a coverage
    /// statement must never trip a round to findings on its own.
    #[must_use]
    pub fn severity(&self) -> u8 {
        match self {
            Self::CleanExact => 0,
            Self::CleanDrifted { .. } => 1,
            Self::Unassessed { .. } => 2,
            Self::Conflict { .. } => 3,
        }
    }

    #[must_use]
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }

    /// Fold another observation of the same path into this outcome,
    /// keeping the worst. Two drifts keep the largest-magnitude delta;
    /// two conflicts keep the worse kind. Ties break on the payload so
    /// the fold is order-independent.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::CleanDrifted { line_delta: a }, Self::CleanDrifted { line_delta: b }) => {
                Self::CleanDrifted {
                    line_delta: if (b.abs(), b) > (a.abs(), a) { b } else { a },
                }
            }
            (Self::Unassessed { reason: a }, Self::Unassessed { reason: b }) => {
                Self::Unassessed { reason: a.max(b) }
            }
            (Self::Conflict { kind: a }, Self::Conflict { kind: b }) => {
                Self::Conflict { kind: a.max(b) }
            }
            (a, b) => {
                if b.severity() > a.severity() {
                    b
                } else {
                    a
                }
            }
        }
    }
}

/// Per-path apply prediction for one `(commit, series)` row, computed
/// from the target tree. Present exactly when a target context was
/// supplied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ApplyForecast {
    /// Every assessed touched path, including ones absent on the
    /// target (`Conflict { FileAbsent }`).
    pub paths: BTreeMap<RelativePath, PathApplyOutcome>,
}

/// Derived-on-read counts over a forecast, never serialized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ApplySummary {
    pub clean: usize,
    pub drifted: usize,
    pub unassessed: usize,
    pub conflicted: usize,
}

impl ApplyForecast {
    /// Record one observation of `path`, folding into any prior
    /// observation at max severity.
    pub fn record(&mut self, path: RelativePath, outcome: PathApplyOutcome) {
        match self.paths.entry(path) {
            Entry::Vacant(slot) => {
                slot.insert(outcome);
            }
            Entry::Occupied(mut slot) => {
                let merged = slot.get().clone().merge(outcome);
                slot.insert(merged);
            }
        }
    }

    #[must_use]
    pub fn has_conflict(&self) -> bool {
        self.paths.values().any(PathApplyOutcome::is_conflict)
    }

    /// Every conflicting path with its conflict kind.
    pub fn conflicts(&self) -> impl Iterator<Item = (&RelativePath, ApplyConflictKind)> {
        self.paths.iter().filter_map(|(p, o)| match o {
            PathApplyOutcome::Conflict { kind } => Some((p, *kind)),
            _ => None,
        })
    }

    /// The worst outcome across paths, by severity.
    #[must_use]
    pub fn worst(&self) -> Option<&PathApplyOutcome> {
        self.paths.values().max_by_key(|o| o.severity())
    }

    #[must_use]
    pub fn summary(&self) -> ApplySummary {
        let mut s = ApplySummary::default();
        for outcome in self.paths.values() {
            match outcome {
                PathApplyOutcome::CleanExact => s.clean += 1,
                PathApplyOutcome::CleanDrifted { .. } => s.drifted += 1,
                PathApplyOutcome::Unassessed { .. } => s.unassessed += 1,
                PathApplyOutcome::Conflict { .. } => s.conflicted += 1,
            }
        }
        s
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}
