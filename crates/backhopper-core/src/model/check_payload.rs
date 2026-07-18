// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The canonical `check`-family wire payload. One definition the CLI
//! serializes, the driver parses, and `schema show` documents, so the
//! three cannot drift apart.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::apply::ApplyForecast;
use crate::model::batch::PinPayload;
use crate::model::evaluation::{
    AggregateVerdict, BehaviourModuleMissingFinding, HeaderFileMissingFinding,
    SeriesEvaluationView, TestModuleSymbolMissingFinding, VersionedMachineSnapshotMissingFinding,
    WireConstantBindingsMissingFinding,
};
use crate::model::findings::TargetFindings;
use crate::model::fingerprint::VerdictFingerprint;
use crate::model::names::{ModuleName, ProjectName, RelativePath, SeriesName, TagName};
use crate::model::pin::Pin;
use crate::model::pr_commit::PrCommit;
use crate::model::resolver_coverage::ResolverCoverage;
use crate::model::summary::VerdictKind;
use crate::model::verdict::{Diagnostics, PinVerdict, Reason, SeriesVerdict};

/// A project the untracked-call analysis suggests adding to config so its
/// calls become tracked-dependency evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ProjectSuggestion {
    pub name: String,
    pub modules: Vec<String>,
    pub call_sites: usize,
}

/// What a `check`-family verb was queried against: a single pin or a
/// whole series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueriedAgainst {
    /// `check ... --project NAME --tag TAG`.
    Pin { project: ProjectName, tag: TagName },
    /// `check ... --series NAME`.
    Series {
        name: SeriesName,
        pins: Vec<PinPayload>,
    },
}

/// The parsed payload of every `check`-family verb.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CheckPayload {
    /// What the verb was queried against (a series or a pin).
    pub queried_against: QueriedAgainst,
    /// Per-pin verdicts plus summary.
    pub results: SeriesVerdict,
    /// Series-wide diagnostics (untracked calls, suggested suites).
    #[serde(default, skip_serializing_if = "Diagnostics::is_empty")]
    pub diagnostics: Diagnostics,
    /// Projects the untracked-call analysis suggests adding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_suggestions: Vec<ProjectSuggestion>,
    /// Inner PR-branch commits for 2-parent merge inputs, `None` for
    /// everything else. The `None` vs `Some(vec![])` distinction is
    /// part of the wire contract, so no `skip_serializing_if`.
    #[serde(default)]
    pub pr_commits: Option<Vec<PrCommit>>,
    /// Projects excluded from the tracked-dependency tally. Always
    /// emitted so a consumer can tell an empty set from an old binary.
    #[serde(default)]
    pub self_projects: Option<BTreeSet<ProjectName>>,
    /// Stable join key for this verdict, omitted for inputs with no
    /// cache key (patch, range, PR, or an unresolvable pin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict_fingerprint: Option<VerdictFingerprint>,
    /// Symbol classes this binary checks, for the measurement fold.
    /// Always emitted so a consumer tells it from an old binary.
    #[serde(default)]
    pub resolver_coverage: Option<ResolverCoverage>,
    /// The fingerprint generation this binary stamped. Always emitted.
    #[serde(default)]
    pub fingerprint_version: Option<u32>,
    /// Apply-axis prediction. Absent when no target context was
    /// supplied: the axis was not evaluated, never "clean".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply: Option<ApplyForecast>,
    /// Symbol-axis target findings, `None` when no target context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_findings: Option<TargetFindings>,
}

impl CheckPayload {
    /// A borrowed view exposing every query and finding accessor. The
    /// same view is reachable from a batch row via
    /// `BatchResult::evaluation`, so both check paths read alike.
    #[must_use]
    pub fn view(&self) -> SeriesEvaluationView<'_> {
        SeriesEvaluationView::new(&self.results, &self.diagnostics)
    }

    /// Self-excluded tracked-dependency count from the wire-recorded
    /// self-projects. `None` when the producer predates the field.
    #[must_use]
    pub fn tracked_refs(&self) -> Option<u32> {
        self.self_projects
            .as_ref()
            .map(|projects| self.view().tracked_refs(projects))
    }

    /// See [`SeriesEvaluationView::worst_verdict`].
    #[must_use]
    pub fn worst_verdict(&self) -> AggregateVerdict {
        self.view().worst_verdict()
    }

    /// See [`SeriesEvaluationView::has_blocking_reason`].
    #[must_use]
    pub fn has_blocking_reason(&self) -> bool {
        self.view().has_blocking_reason()
    }

    /// See [`SeriesEvaluationView::pins_in`].
    pub fn pins_in(&self, verdict: VerdictKind) -> impl Iterator<Item = &PinVerdict> {
        self.view().pins_in(verdict)
    }

    /// See [`SeriesEvaluationView::reasons_for`].
    pub fn reasons_for(&self, pin: &Pin) -> impl Iterator<Item = &Reason> {
        self.view().reasons_for(pin)
    }

    /// See [`SeriesEvaluationView::pin_by_project`].
    #[must_use]
    pub fn pin_by_project(&self, project: &ProjectName) -> Option<&PinVerdict> {
        self.view().pin_by_project(project)
    }

    /// See [`SeriesEvaluationView::missing_test_modules`].
    #[must_use]
    pub fn missing_test_modules(&self) -> &BTreeMap<RelativePath, BTreeMap<ModuleName, usize>> {
        self.view().missing_test_modules()
    }

    /// See [`SeriesEvaluationView::test_module_symbol_missing`].
    pub fn test_module_symbol_missing(
        &self,
    ) -> impl Iterator<Item = TestModuleSymbolMissingFinding<'_>> {
        self.view().test_module_symbol_missing()
    }

    /// See [`SeriesEvaluationView::header_file_missing`].
    pub fn header_file_missing(&self) -> impl Iterator<Item = HeaderFileMissingFinding<'_>> {
        self.view().header_file_missing()
    }

    /// See [`SeriesEvaluationView::behaviour_module_missing`].
    pub fn behaviour_module_missing(
        &self,
    ) -> impl Iterator<Item = BehaviourModuleMissingFinding<'_>> {
        self.view().behaviour_module_missing()
    }

    /// See [`SeriesEvaluationView::versioned_machine_snapshot_missing`].
    pub fn versioned_machine_snapshot_missing(
        &self,
    ) -> impl Iterator<Item = VersionedMachineSnapshotMissingFinding<'_>> {
        self.view().versioned_machine_snapshot_missing()
    }

    /// See [`SeriesEvaluationView::wire_constant_bindings_missing`].
    pub fn wire_constant_bindings_missing(
        &self,
    ) -> impl Iterator<Item = WireConstantBindingsMissingFinding<'_>> {
        self.view().wire_constant_bindings_missing()
    }
}
