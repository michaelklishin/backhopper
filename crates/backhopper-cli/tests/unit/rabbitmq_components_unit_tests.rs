// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::str::FromStr;

use backhopper_cli::commands::rabbitmq_components::{
    dep_to_tag, series_name_for_branch, version_to_tag,
};
use backhopper_core::erlang_mk::{DepPin, DepSource};
use backhopper_core::model::names::{DependencyName, DependencyVersion};

fn pin(name: &str, source: DepSource, version: &str) -> DepPin {
    DepPin {
        name: DependencyName::from_str(name).unwrap(),
        source,
        version: DependencyVersion::from_str(version).unwrap(),
    }
}

#[test]
fn series_name_for_branch_strips_version_suffix() {
    assert_eq!(series_name_for_branch("v4.2.x"), "rabbitmq-4.2");
    assert_eq!(series_name_for_branch("v4.3.x"), "rabbitmq-4.3");
    assert_eq!(series_name_for_branch("main"), "rabbitmq-main");
    assert_eq!(series_name_for_branch("master"), "rabbitmq-master");
    assert_eq!(series_name_for_branch("refs/heads/v4.2.x"), "rabbitmq-4.2");
}

#[test]
fn version_to_tag_applies_prefix_only_when_missing() {
    assert_eq!(version_to_tag("3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("v3.1.6", "v"), "v3.1.6");
    assert_eq!(version_to_tag("2.14.1", ""), "2.14.1");
}

#[test]
fn dep_to_tag_uses_prefix_for_hex_and_verbatim_for_git() {
    let hex = pin("ra", DepSource::Hex, "2.16.7");
    assert_eq!(dep_to_tag(&hex, "v"), "v2.16.7");
    assert_eq!(dep_to_tag(&hex, ""), "2.16.7");

    let git = pin("osiris", DepSource::Git, "v1.10.3");
    assert_eq!(
        dep_to_tag(&git, "v"),
        "v1.10.3",
        "git deps use the literal version regardless of prefix"
    );

    let git_rmq = pin("cowboy", DepSource::GitRmq, "2.13.0");
    assert_eq!(dep_to_tag(&git_rmq, "v"), "2.13.0");
}
