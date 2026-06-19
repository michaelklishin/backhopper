// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Ergonomic, borrowed view over a `SeriesVerdict` and its `Diagnostics`.
//!
//! The check verbs and `check batch` carry the same verdict and diagnostic
//! payloads in different envelopes. This view holds the query and finding
//! accessors once so both paths read identically: a `BatchResult` row reaches
//! it through [`BatchResult::evaluation`], and the driver's single-check
//! `SeriesEvaluation` through its own `view`.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::batch::BatchResult;
use crate::model::names::{MacroName, ModuleName, ProjectName, RelativePath};
use crate::model::pin::Pin;
use crate::model::summary::VerdictKind;
use crate::model::verdict::{
    Diagnostics, IncludeDirective, PinVerdict, Reason, SeriesVerdict, SnapshotSide, TestCallSite,
    Verdict, non_self_tracked,
};

/// Aggregate verdict across the pins of a series.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggregateVerdict {
    /// At least one pin compatible, none worse.
    Compatible,
    /// Some pin requires adaptation, none incompatible.
    RequiresAdaptation,
    /// At least one pin incompatible.
    Incompatible,
    /// All pins inapplicable.
    Inapplicable,
    /// The series has no pins.
    Empty,
}

/// Borrowed accessors over a `SeriesVerdict` and its `Diagnostics`.
///
/// Methods take `self` by value; the view is `Copy`, so a temporary like
/// `row.evaluation().worst_verdict()` is cheap and the returned iterators
/// borrow the underlying data rather than the view.
#[derive(Debug, Clone, Copy)]
pub struct SeriesEvaluationView<'a> {
    results: &'a SeriesVerdict,
    diagnostics: &'a Diagnostics,
}

impl<'a> SeriesEvaluationView<'a> {
    /// Construct a view over a verdict and its diagnostics.
    #[must_use]
    pub fn new(results: &'a SeriesVerdict, diagnostics: &'a Diagnostics) -> Self {
        Self {
            results,
            diagnostics,
        }
    }

    /// Worst aggregate verdict across pins. `Empty` when the series has no
    /// pins.
    #[must_use]
    pub fn worst_verdict(self) -> AggregateVerdict {
        if self.results.results.is_empty() {
            return AggregateVerdict::Empty;
        }
        let s = &self.results.summary;
        if s.incompatible > 0 {
            AggregateVerdict::Incompatible
        } else if s.requires_adaptation > 0 {
            AggregateVerdict::RequiresAdaptation
        } else if s.compatible > 0 {
            AggregateVerdict::Compatible
        } else if s.inapplicable > 0 {
            AggregateVerdict::Inapplicable
        } else {
            AggregateVerdict::Empty
        }
    }

    /// `true` if any pin carries a blocking reason.
    #[must_use]
    pub fn has_blocking_reason(self) -> bool {
        self.results
            .results
            .iter()
            .any(|p| matches!(&p.verdict, Verdict::Incompatible { .. }))
    }

    /// Iterate over pin verdicts whose verdict matches `verdict`.
    pub fn pins_in(self, verdict: VerdictKind) -> impl Iterator<Item = &'a PinVerdict> {
        self.results
            .results
            .iter()
            .filter(move |p| VerdictKind::from(&p.verdict) == verdict)
    }

    /// All [`Reason`] values attached to the pin matching `pin`.
    pub fn reasons_for(self, pin: &Pin) -> impl Iterator<Item = &'a Reason> {
        self.results
            .results
            .iter()
            .filter(move |p| &p.pin == pin)
            .flat_map(pin_reasons)
    }

    /// Saturating sum of `tracked_refs` over non-self pins.
    #[must_use]
    pub fn tracked_refs(self, self_projects: &BTreeSet<ProjectName>) -> u32 {
        non_self_tracked(&self.results.results, self_projects)
    }

    /// First pin whose project name matches `project`, if any.
    #[must_use]
    pub fn pin_by_project(self, project: &ProjectName) -> Option<&'a PinVerdict> {
        self.results
            .results
            .iter()
            .find(|p| &p.pin.project == project)
    }

    /// Always-on diagnostic counterpart of `Reason::TestModuleSymbolMissing`
    /// from `018_ci_signal_gaps`: a per-`_SUITE.erl` map of
    /// `helper_module -> call_site_count`. Empty when the patch did not touch
    /// a SUITE that references an unresolved helper.
    #[must_use]
    pub fn missing_test_modules(self) -> &'a BTreeMap<RelativePath, BTreeMap<ModuleName, usize>> {
        &self.diagnostics.missing_test_modules
    }

    /// Iterate every `Reason::TestModuleSymbolMissing` flattened to
    /// `(pin, suite_path, missing_module, call_sites)`, so consumers surface
    /// the signal without re-deriving it from the `Reason` enum.
    pub fn test_module_symbol_missing(
        self,
    ) -> impl Iterator<Item = TestModuleSymbolMissingFinding<'a>> {
        self.results
            .results
            .iter()
            .flat_map(|pin| pin_reasons(pin).map(move |r| (pin, r)))
            .filter_map(|(pin, r)| match r {
                Reason::TestModuleSymbolMissing {
                    suite_path,
                    missing_module,
                    call_sites,
                } => Some(TestModuleSymbolMissingFinding {
                    pin,
                    suite_path,
                    missing_module,
                    call_sites: call_sites.as_slice(),
                }),
                _ => None,
            })
    }

    /// Iterate every `Reason::HeaderFileMissing` flattened to
    /// `(pin, source_path, include_directive, attempted_paths)`.
    pub fn header_file_missing(self) -> impl Iterator<Item = HeaderFileMissingFinding<'a>> {
        self.results
            .results
            .iter()
            .flat_map(|pin| pin_reasons(pin).map(move |r| (pin, r)))
            .filter_map(|(pin, r)| match r {
                Reason::HeaderFileMissing {
                    source_path,
                    include_directive,
                    attempted_paths,
                } => Some(HeaderFileMissingFinding {
                    pin,
                    source_path,
                    include_directive,
                    attempted_paths: attempted_paths.as_slice(),
                }),
                _ => None,
            })
    }

    /// Iterate every `Reason::BehaviourModuleMissing` flattened to
    /// `(pin, source_path, behaviour)`.
    pub fn behaviour_module_missing(
        self,
    ) -> impl Iterator<Item = BehaviourModuleMissingFinding<'a>> {
        self.results
            .results
            .iter()
            .flat_map(|pin| pin_reasons(pin).map(move |r| (pin, r)))
            .filter_map(|(pin, r)| match r {
                Reason::BehaviourModuleMissing {
                    source_path,
                    behaviour,
                } => Some(BehaviourModuleMissingFinding {
                    pin,
                    source_path,
                    behaviour,
                }),
                _ => None,
            })
    }

    /// Iterate every `Reason::VersionedMachineSnapshotMissing` flattened to
    /// `(pin, module, side)`.
    pub fn versioned_machine_snapshot_missing(
        self,
    ) -> impl Iterator<Item = VersionedMachineSnapshotMissingFinding<'a>> {
        self.results
            .results
            .iter()
            .flat_map(|pin| pin_reasons(pin).map(move |r| (pin, r)))
            .filter_map(|(pin, r)| match r {
                Reason::VersionedMachineSnapshotMissing { module, side } => {
                    Some(VersionedMachineSnapshotMissingFinding {
                        pin,
                        module,
                        side: *side,
                    })
                }
                _ => None,
            })
    }

    /// Iterate every `Reason::WireConstantBindingsMissing` flattened to
    /// `(pin, module, macros, side)`.
    pub fn wire_constant_bindings_missing(
        self,
    ) -> impl Iterator<Item = WireConstantBindingsMissingFinding<'a>> {
        self.results
            .results
            .iter()
            .flat_map(|pin| pin_reasons(pin).map(move |r| (pin, r)))
            .filter_map(|(pin, r)| match r {
                Reason::WireConstantBindingsMissing {
                    module,
                    macros,
                    side,
                } => Some(WireConstantBindingsMissingFinding {
                    pin,
                    module,
                    macros: macros.as_slice(),
                    side: *side,
                }),
                _ => None,
            })
    }
}

impl BatchResult {
    /// A borrowed [`SeriesEvaluationView`] over this row's verdict and
    /// diagnostics, exposing the same accessors as a single-check evaluation.
    /// The batch-only fields (`patch_facts`, `touched_paths`, `pr_commits`,
    /// `parent_count`) stay on the row.
    #[must_use]
    pub fn evaluation(&self) -> SeriesEvaluationView<'_> {
        SeriesEvaluationView::new(&self.verdict, &self.diagnostics)
    }
}

/// Borrowed view of one `Reason::TestModuleSymbolMissing` with its owning pin.
#[derive(Debug, Clone, Copy)]
pub struct TestModuleSymbolMissingFinding<'a> {
    /// The pin whose verdict carries this reason.
    pub pin: &'a PinVerdict,
    /// The `_SUITE.erl` path the suite is at on the source side.
    pub suite_path: &'a RelativePath,
    /// The unresolved helper module the suite references.
    pub missing_module: &'a ModuleName,
    /// Every call site inside the suite that reaches the missing helper.
    pub call_sites: &'a [TestCallSite],
}

/// Borrowed view of one `Reason::HeaderFileMissing` with its owning pin.
#[derive(Debug, Clone, Copy)]
pub struct HeaderFileMissingFinding<'a> {
    /// The pin whose verdict carries this reason.
    pub pin: &'a PinVerdict,
    /// The touched `.erl` or `.hrl` that declared the unresolved include.
    pub source_path: &'a RelativePath,
    /// The include directive verbatim.
    pub include_directive: &'a IncludeDirective,
    /// Target paths the resolver tried in order.
    pub attempted_paths: &'a [RelativePath],
}

/// Borrowed view of one `Reason::BehaviourModuleMissing` with its owning pin.
#[derive(Debug, Clone, Copy)]
pub struct BehaviourModuleMissingFinding<'a> {
    /// The pin whose verdict carries this reason.
    pub pin: &'a PinVerdict,
    /// The touched `.erl` that declared `-behaviour(behaviour)`.
    pub source_path: &'a RelativePath,
    /// The unresolved behaviour module name.
    pub behaviour: &'a ModuleName,
}

/// Borrowed view of one `Reason::VersionedMachineSnapshotMissing` with its
/// owning pin.
#[derive(Debug, Clone, Copy)]
pub struct VersionedMachineSnapshotMissingFinding<'a> {
    /// The pin whose verdict carries this reason.
    pub pin: &'a PinVerdict,
    /// The declared versioned-machine module the data is missing for.
    pub module: &'a ModuleName,
    /// Which snapshot side lacks the data.
    pub side: SnapshotSide,
}

/// Borrowed view of one `Reason::WireConstantBindingsMissing` with its owning
/// pin.
#[derive(Debug, Clone, Copy)]
pub struct WireConstantBindingsMissingFinding<'a> {
    /// The pin whose verdict carries this reason.
    pub pin: &'a PinVerdict,
    /// The declared module whose wire-constant bindings are missing.
    pub module: &'a ModuleName,
    /// The macros that lack a binding on `side`, sorted alphabetically.
    pub macros: &'a [MacroName],
    /// Which snapshot side lacks the bindings.
    pub side: SnapshotSide,
}

fn pin_reasons(pin: &PinVerdict) -> impl Iterator<Item = &Reason> {
    match &pin.verdict {
        Verdict::Compatible | Verdict::Inapplicable { .. } => [].iter(),
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            reasons.iter()
        }
    }
}
