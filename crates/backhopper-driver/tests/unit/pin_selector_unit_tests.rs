// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_driver::PinSelector;
use backhopper_driver::types::{ProjectName, SeriesName, TagName};

#[test]
fn series_constructor_yields_series_variant() {
    let s = SeriesName::from_str("rabbitmq-4.2").unwrap();
    let sel = PinSelector::series(s.clone());
    assert!(matches!(sel, PinSelector::Series(name) if name == s));
}

#[test]
fn pin_constructor_yields_pin_variant() {
    let project = ProjectName::from_str("ra").unwrap();
    let tag = TagName::from_str("v2.16.7").unwrap();
    let sel = PinSelector::pin(project.clone(), tag.clone());
    match sel {
        PinSelector::Pin { project: p, tag: t } => {
            assert_eq!(p, project);
            assert_eq!(t, tag);
        }
        other => panic!("expected Pin, got {other:?}"),
    }
}

#[test]
fn from_series_name_into_selector() {
    let s = SeriesName::from_str("rabbitmq-4.3").unwrap();
    let sel: PinSelector = s.into();
    assert!(matches!(sel, PinSelector::Series(_)));
}

#[test]
fn from_project_tag_tuple_into_selector() {
    let project = ProjectName::from_str("khepri").unwrap();
    let tag = TagName::from_str("v0.17.5").unwrap();
    let sel: PinSelector = (project, tag).into();
    assert!(matches!(sel, PinSelector::Pin { .. }));
}
