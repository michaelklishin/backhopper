// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `non_self_tracked`: the `tracked_refs` sum over non-self pins, and
//! the matching `SeriesEvaluationView::tracked_refs` accessor.

use std::collections::BTreeSet;

use backhopper_core::model::evaluation::SeriesEvaluationView;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    Diagnostics, PinVerdict, SeriesVerdict, Verdict, non_self_tracked,
};

fn proj(name: &str) -> ProjectName {
    ProjectName::new(name).unwrap()
}

fn pin(project: &str, tracked: usize) -> PinVerdict {
    PinVerdict::new(
        Pin::new(proj(project), TagName::new("v1.0.0").unwrap()),
        Verdict::Compatible,
    )
    .with_tracked_refs(tracked)
}

fn self_set(names: &[&str]) -> BTreeSet<ProjectName> {
    names.iter().map(|n| proj(n)).collect()
}

#[test]
fn no_self_projects_sums_every_pin() {
    let pins = vec![pin("ra", 3), pin("khepri", 4)];
    assert_eq!(non_self_tracked(&pins, &BTreeSet::new()), 7);
}

#[test]
fn one_self_project_is_excluded() {
    let pins = vec![pin("rabbit", 9), pin("ra", 4)];
    assert_eq!(non_self_tracked(&pins, &self_set(&["rabbit"])), 4);
}

#[test]
fn multiple_self_projects_are_all_excluded() {
    let pins = vec![pin("rabbit", 9), pin("rabbit_common", 2), pin("ra", 5)];
    assert_eq!(
        non_self_tracked(&pins, &self_set(&["rabbit", "rabbit_common"])),
        5
    );
}

#[test]
fn sum_saturates_at_u32_max() {
    let huge = usize::try_from(u32::MAX).unwrap() + 1;
    let pins = vec![pin("ra", huge), pin("khepri", huge)];
    assert_eq!(non_self_tracked(&pins, &BTreeSet::new()), u32::MAX);
}

#[test]
fn view_tracked_refs_matches_the_free_function() {
    let pins = vec![pin("rabbit", 9), pin("ra", 4), pin("khepri", 2)];
    let verdict = SeriesVerdict::from_results(pins.clone());
    let diagnostics = Diagnostics::default();
    let view = SeriesEvaluationView::new(&verdict, &diagnostics);
    let self_projects = self_set(&["rabbit"]);
    assert_eq!(
        view.tracked_refs(&self_projects),
        non_self_tracked(&pins, &self_projects)
    );
    assert_eq!(view.tracked_refs(&self_projects), 6);
}
