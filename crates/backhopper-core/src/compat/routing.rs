// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Per-pin path routing: which configured project owns each touched
//! path, so a pin's verdict can promote to `OutOfScopeFor` or
//! `Untracked` instead of firing a misleading `FileAbsent` row.

use std::collections::BTreeSet;
use std::mem;
use std::path::{Path, PathBuf};

use crate::config::Project;
use crate::model::names::ProjectName;
use crate::model::verdict::{InapplicableReason, PinVerdict, SeriesVerdict, Verdict};

impl SeriesVerdict {
    /// The only way to assemble a series verdict from routed pins:
    /// an unrouted `PinVerdict` cannot reach the envelope.
    pub fn from_routed(results: Vec<RoutedPinVerdict>) -> Self {
        Self::from_results(
            results
                .into_iter()
                .map(RoutedPinVerdict::into_inner)
                .collect(),
        )
    }
}

/// Path-routing summary for one pin's evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathRouting {
    pub any_in_scope: bool,
    pub out_of_scope_owners: BTreeSet<ProjectName>,
    pub untracked_count: usize,
}

impl PathRouting {
    pub fn first_out_of_scope_owner(&self) -> Option<&ProjectName> {
        self.out_of_scope_owners.first()
    }

    pub fn has_any_attribution(&self) -> bool {
        !self.out_of_scope_owners.is_empty() || self.untracked_count > 0
    }
}

/// True when path is under scan_paths, app_roots, or `deps/{name}/`;
/// projects with no scope configured fall back to "owns everything".
pub fn project_owns_path(project: &Project, path: &Path) -> bool {
    project_owns_candidate(project, &path.to_string_lossy())
}

fn project_owns_candidate(project: &Project, candidate: &str) -> bool {
    if project.owns_path(Path::new(candidate)) {
        return true;
    }
    if !project.is_self()
        && let Some(rest) = candidate.strip_prefix("deps/")
        && let Some(rest) = rest.strip_prefix(project.name.as_str())
        && rest.starts_with('/')
    {
        return true;
    }
    project.scan_paths.is_empty() && project.app_roots.is_empty()
}

pub fn classify_paths_for_pin(
    touched: &[PathBuf],
    pin_project: &Project,
    siblings: &[&Project],
) -> PathRouting {
    let mut routing = PathRouting::default();
    for p in touched {
        let candidate = p.to_string_lossy();
        if project_owns_candidate(pin_project, &candidate) {
            routing.any_in_scope = true;
            continue;
        }
        let owner = siblings
            .iter()
            .find(|s| s.name != pin_project.name && project_owns_candidate(s, &candidate))
            .map(|s| s.name.clone());
        match owner {
            Some(name) => {
                routing.out_of_scope_owners.insert(name);
            }
            None => routing.untracked_count += 1,
        }
    }
    routing
}

/// A pin verdict that has been through path routing exactly once.
/// `SeriesVerdict::from_routed` is the only consumer, so an unrouted
/// verdict cannot reach the envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedPinVerdict(PinVerdict);

impl RoutedPinVerdict {
    #[must_use]
    pub fn into_inner(self) -> PinVerdict {
        self.0
    }
}

/// Path routing, reason-granular. File ownership gates only the
/// path-scoped reason family; reference-level evidence is kept no
/// matter which file holds the call site. A pin with no owned
/// paths and no evidence rewrites to `Inapplicable`.
#[must_use]
pub fn route_pin_verdict(mut pv: PinVerdict, routing: &PathRouting) -> RoutedPinVerdict {
    if routing.any_in_scope || !routing.has_any_attribution() {
        return RoutedPinVerdict(pv);
    }
    let inapplicable_reason = |pv: &PinVerdict| {
        pv.touched.inapplicable_reason().unwrap_or_else(|| {
            match routing.first_out_of_scope_owner() {
                Some(name) => InapplicableReason::OutOfScopeFor {
                    project: name.clone(),
                },
                None => InapplicableReason::Untracked,
            }
        })
    };
    let verdict = mem::replace(&mut pv.verdict, Verdict::Compatible);
    pv.verdict = match verdict {
        Verdict::Inapplicable { .. } => verdict,
        Verdict::Compatible => {
            if pv.tracked_refs > 0 {
                Verdict::Compatible
            } else {
                Verdict::Inapplicable {
                    reason: inapplicable_reason(&pv),
                }
            }
        }
        Verdict::RequiresAdaptation { reasons } | Verdict::Incompatible { reasons } => {
            let surviving: Vec<_> = reasons
                .into_iter()
                .filter(|r| !r.is_path_scoped())
                .collect();
            if surviving.is_empty() && pv.tracked_refs == 0 {
                Verdict::Inapplicable {
                    reason: inapplicable_reason(&pv),
                }
            } else {
                Verdict::from_reasons(surviving)
            }
        }
    };
    RoutedPinVerdict(pv)
}
