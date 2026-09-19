// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `PinSelectorArgs` parses into a `PinSelector` at the CLI boundary: two
//! valid triples convert, the other six are refused.

use std::str::FromStr;

use backhopper_cli::cli::check::PinSelectorArgs;
use backhopper_core::model::names::{ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::PinSelector;
use proptest::prelude::*;

fn args(project: Option<&str>, tag: Option<&str>, series: Option<&str>) -> PinSelectorArgs {
    PinSelectorArgs {
        project: project.map(|p| ProjectName::from_str(p).unwrap()),
        tag: tag.map(|t| TagName::from_str(t).unwrap()),
        series: series.map(|s| SeriesName::from_str(s).unwrap()),
    }
}

#[test]
fn project_and_tag_convert_to_a_pin_selector() {
    let selector = PinSelector::try_from(args(Some("ra"), Some("v2.16.7"), None)).unwrap();
    match selector {
        PinSelector::Pin { project, tag } => {
            assert_eq!(project.as_str(), "ra");
            assert_eq!(tag.as_str(), "v2.16.7");
        }
        other @ PinSelector::Series(_) => panic!("expected Pin, got {other:?}"),
    }
}

#[test]
fn series_alone_converts_to_a_series_selector() {
    let selector = PinSelector::try_from(args(None, None, Some("rabbitmq-4.2"))).unwrap();
    assert!(matches!(selector, PinSelector::Series(name) if name.as_str() == "rabbitmq-4.2"));
}

#[test]
fn nothing_set_is_refused() {
    assert!(PinSelector::try_from(args(None, None, None)).is_err());
}

#[test]
fn project_alone_without_tag_is_refused() {
    assert!(PinSelector::try_from(args(Some("ra"), None, None)).is_err());
}

#[test]
fn tag_alone_without_project_is_refused() {
    assert!(PinSelector::try_from(args(None, Some("v2.16.7"), None)).is_err());
}

#[test]
fn project_and_series_together_is_refused() {
    assert!(PinSelector::try_from(args(Some("ra"), None, Some("rabbitmq-4.2"))).is_err());
}

#[test]
fn tag_and_series_together_is_refused() {
    assert!(PinSelector::try_from(args(None, Some("v2.16.7"), Some("rabbitmq-4.2"))).is_err());
}

#[test]
fn project_tag_and_series_all_set_is_refused() {
    assert!(
        PinSelector::try_from(args(Some("ra"), Some("v2.16.7"), Some("rabbitmq-4.2"))).is_err()
    );
}

proptest! {
    #[test]
    fn try_from_accepts_exactly_the_triples_clap_would_have_accepted(
        has_project in any::<bool>(),
        has_tag in any::<bool>(),
        has_series in any::<bool>(),
    ) {
        let selector = PinSelectorArgs {
            project: has_project.then(|| ProjectName::from_str("ra").unwrap()),
            tag: has_tag.then(|| TagName::from_str("v2.16.7").unwrap()),
            series: has_series.then(|| SeriesName::from_str("rabbitmq-4.2").unwrap()),
        };
        // clap's `conflicts_with`/`requires` admit exactly two states out of
        // the eight: (project, tag, no series) and (no project, no tag, series).
        let clap_would_accept =
            (has_project && has_tag && !has_series) || (!has_project && !has_tag && has_series);
        prop_assert_eq!(PinSelector::try_from(selector).is_ok(), clap_would_accept);
    }
}
