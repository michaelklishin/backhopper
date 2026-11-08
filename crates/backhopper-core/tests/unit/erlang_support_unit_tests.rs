// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::release_version::HostVersion;

use crate::jigsaw_support::{config, rabbitmq};

fn range_for(release: &str) -> String {
    let cfg = config();
    rabbitmq(&cfg)
        .erlang_support
        .range_for(&release.parse::<HostVersion>().unwrap())
        .to_string()
}

#[test]
fn a_release_between_entries_takes_the_nearest_one_below() {
    assert_eq!(range_for("4.2.8"), "[26.2,29.0)");
}

#[test]
fn a_release_above_the_last_entry_takes_the_last_one() {
    assert_eq!(range_for("4.2.11"), "[27.0,29.0)");
}

#[test]
fn a_listed_release_takes_its_own_entry() {
    assert_eq!(range_for("4.2.0"), "[26.2,29.0)");
    assert_eq!(range_for("4.2.9"), "[27.0,29.0)");
}

#[test]
fn a_release_below_the_first_entry_takes_the_default() {
    assert_eq!(range_for("4.1.9"), "[27.0,30.0)");
}
