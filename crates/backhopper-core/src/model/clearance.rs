// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Round-level roll-up of a `check batch` result set. Derived on read
//! from the rows, never serialized: it carries no schema version and
//! no wire field is added.
//!
//! The point is trust. A renderer cannot report any outcome without
//! going through the matching arm and stating what did or did not
//! happen. `Clean` states analyzable surface was touched and passed;
//! `ZeroDomain` states backhopper's dep and symbol domain did not
//! intersect the round at all; `Findings` names what needs review.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::apply::PathApplyOutcome;
use crate::model::batch::BatchResult;
use crate::model::names::{ProjectName, RelativePath};
use crate::model::resolver_coverage::ResolverClass;
use crate::model::verdict::{BumpStatus, SeriesSummary, Verdict, exit, non_self_tracked};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundClearance {
    /// No dep-API risk; at least one candidate touched analyzable surface.
    Clean(ClearanceFacts),
    /// Every candidate was inapplicable; backhopper's dep and symbol domain
    /// did not intersect this round.
    ZeroDomain(ClearanceFacts),
    /// A reviewer should review: a blocking verdict, tracked-dep references,
    /// or a snapshot-missing pin bump.
    Findings(ClearanceFacts),
}

/// The facts behind a clearance, shared by both arms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearanceFacts {
    /// Distinct commits across the result set.
    pub candidates: usize,
    /// Distinct series across the result set.
    pub series: usize,
    /// Total `(commit, series)` rows.
    pub rows: usize,
    /// Tracked-dep references summed over non-self pins, saturating.
    pub tracked: u32,
    /// Per-pin verdict counts summed over rows, matching the batch
    /// totals line.
    pub verdicts: SeriesSummary,
    pub reasons: ReasonHistogram,
    pub bumps: BumpSummary,
    /// Apply-axis roll-up. `rows_with_forecast == 0` means no row
    /// carried a forecast, and the renderer says so.
    pub apply: ApplyRollup,
    /// Symbol-axis roll-up. `rows_with_findings == 0` means no row
    /// carried the field, and the renderer says so.
    pub target: TargetFindingsRollup,
    /// Union of every row's `suggested_suites`, sorted and deduplicated.
    pub suites: Vec<String>,
    /// `exit::OK` or `exit::NEEDS_ATTENTION`, the value `check batch`
    /// returns for the round.
    pub exit_code: i32,
}

/// Inapplicability reasons tallied by their stable `as_str()` label.
/// Keyed on the label, not the value, so `OutOfScopeFor { project }`
/// rows with different projects group under one count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReasonHistogram(BTreeMap<&'static str, u32>);

impl ReasonHistogram {
    fn record(&mut self, label: &'static str) {
        *self.0.entry(label).or_insert(0) += 1;
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn total(&self) -> u32 {
        self.0.values().copied().sum()
    }

    #[must_use]
    pub fn count(&self, label: &str) -> u32 {
        self.0.get(label).copied().unwrap_or(0)
    }

    /// Label and count pairs, descending by count then ascending by
    /// label, so the dominant reason reads first.
    #[must_use]
    pub fn ranked(&self) -> Vec<(&'static str, u32)> {
        let mut ranked: Vec<(&'static str, u32)> = self.0.iter().map(|(k, n)| (*k, *n)).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        ranked
    }
}

/// Distinct dep-pin bumps across the round, classified by snapshot
/// status. Deduplicated by `(commit, dep, to)` so a bump seen under
/// several series counts once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BumpSummary {
    pub total: u32,
    pub snapshot_present: u32,
    /// The risk flag: a tracked dep moved to a version with no
    /// snapshot to vet against.
    pub snapshot_missing: u32,
    pub untracked: u32,
    /// Status unfilled, the value a cache hit carries.
    pub unassessed: u32,
    /// Bumps that introduce a pin rather than change one (`from` absent).
    pub introduced: u32,
}

impl BumpSummary {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

/// Apply-axis facts summed over the round's rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyRollup {
    /// Rows that carried an apply forecast at all.
    pub rows_with_forecast: usize,
    /// Rows whose forecast predicts at least one conflict.
    pub conflicted_rows: usize,
    /// Distinct conflicting paths across the round, sorted.
    pub conflicted_paths: Vec<RelativePath>,
    /// Conflict counts by kind label, summed over rows.
    pub by_kind: BTreeMap<&'static str, u32>,
    /// Paths stated unassessed, summed over rows; coverage, not risk.
    pub unassessed_paths: u32,
}

impl ApplyRollup {
    #[must_use]
    pub fn has_conflict(&self) -> bool {
        self.conflicted_rows > 0
    }
}

/// Symbol-axis facts summed over the round's rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetFindingsRollup {
    /// Rows that carried the findings field at all.
    pub rows_with_findings: usize,
    /// Rows whose findings are non-empty.
    pub finding_rows: usize,
    /// Finding counts by resolver class, the ranking view.
    pub by_class: Vec<(ResolverClass, usize)>,
    /// Findings with no resolver class, counted so the class view
    /// never hides a reason.
    pub unclassified: usize,
}

impl TargetFindingsRollup {
    #[must_use]
    pub fn has_findings(&self) -> bool {
        self.finding_rows > 0
    }
}

impl RoundClearance {
    #[must_use]
    pub fn from_results(results: &[BatchResult], self_projects: &BTreeSet<ProjectName>) -> Self {
        let mut commits = BTreeSet::new();
        let mut series = BTreeSet::new();
        let mut tracked: u32 = 0;
        let mut verdicts = SeriesSummary::default();
        let mut reasons = ReasonHistogram::default();
        let mut suites = BTreeSet::new();
        let mut bumps = BumpSummary::default();
        let mut seen_bumps = BTreeSet::new();
        let mut snapshot_missing_bump = false;
        let mut apply = ApplyRollup::default();
        let mut conflicted_paths = BTreeSet::new();
        let mut target = TargetFindingsRollup::default();
        let mut target_by_class: BTreeMap<ResolverClass, usize> = BTreeMap::new();

        for row in results {
            commits.insert(&row.commit);
            series.insert(&row.series);

            verdicts.compatible += row.verdict.summary.compatible;
            verdicts.requires_adaptation += row.verdict.summary.requires_adaptation;
            verdicts.incompatible += row.verdict.summary.incompatible;
            verdicts.inapplicable += row.verdict.summary.inapplicable;

            tracked = tracked.saturating_add(non_self_tracked(&row.verdict.results, self_projects));
            for pin in &row.verdict.results {
                if let Verdict::Inapplicable { reason } = &pin.verdict {
                    reasons.record(reason.as_str());
                }
            }

            for suite in &row.diagnostics.suggested_suites {
                suites.insert(suite.clone());
            }

            if let Some(forecast) = &row.apply {
                apply.rows_with_forecast += 1;
                if forecast.has_conflict() {
                    apply.conflicted_rows += 1;
                }
                for (path, outcome) in &forecast.paths {
                    match outcome {
                        PathApplyOutcome::Conflict { kind } => {
                            conflicted_paths.insert(path.clone());
                            *apply.by_kind.entry(kind.as_str()).or_insert(0) += 1;
                        }
                        PathApplyOutcome::Unassessed { .. } => apply.unassessed_paths += 1,
                        PathApplyOutcome::CleanExact | PathApplyOutcome::CleanDrifted { .. } => {}
                    }
                }
            }

            if let Some(findings) = &row.target_findings {
                target.rows_with_findings += 1;
                if !findings.is_empty() {
                    target.finding_rows += 1;
                }
                for (class, n) in findings.by_class() {
                    *target_by_class.entry(class).or_insert(0) += n;
                }
                target.unclassified += findings.unclassified();
            }

            for bump in &row.diagnostics.pin_bumps {
                if !seen_bumps.insert((&row.commit, &bump.dep, bump.to.as_str())) {
                    continue;
                }
                bumps.total += 1;
                if bump.from.is_none() {
                    bumps.introduced += 1;
                }
                match &bump.status {
                    Some(BumpStatus::SnapshotPresent) => bumps.snapshot_present += 1,
                    Some(BumpStatus::SnapshotMissing { .. }) => {
                        bumps.snapshot_missing += 1;
                        snapshot_missing_bump = true;
                    }
                    Some(BumpStatus::Untracked) => bumps.untracked += 1,
                    None => bumps.unassessed += 1,
                }
            }
        }

        apply.conflicted_paths = conflicted_paths.into_iter().collect();
        target.by_class = target_by_class.into_iter().collect();

        let blocking = verdicts.is_blocking();
        // non-empty findings need attention even when non-blocking
        let exit_code = if blocking || apply.has_conflict() || target.has_findings() {
            exit::NEEDS_ATTENTION
        } else {
            exit::OK
        };

        let facts = ClearanceFacts {
            candidates: commits.len(),
            series: series.len(),
            rows: results.len(),
            tracked,
            verdicts,
            reasons,
            bumps,
            apply,
            target,
            suites: suites.into_iter().collect(),
            exit_code,
        };

        let all_inapplicable = facts.verdicts.compatible == 0
            && facts.verdicts.requires_adaptation == 0
            && facts.verdicts.incompatible == 0
            && facts.verdicts.inapplicable > 0;

        if blocking
            || tracked > 0
            || snapshot_missing_bump
            || facts.apply.has_conflict()
            || facts.target.has_findings()
        {
            Self::Findings(facts)
        } else if all_inapplicable {
            Self::ZeroDomain(facts)
        } else {
            Self::Clean(facts)
        }
    }

    #[must_use]
    pub fn facts(&self) -> &ClearanceFacts {
        match self {
            Self::Clean(f) | Self::ZeroDomain(f) | Self::Findings(f) => f,
        }
    }

    #[must_use]
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Clean(_) | Self::ZeroDomain(_))
    }
}
