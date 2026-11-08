// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;

use backhopper_core::config::PinSource;
use backhopper_core::jigsaw::host_facts_digest;
use backhopper_core::model::names::{
    ApplicationName, DependencyName, DependencyVersion, MakeVariableName, RelativePath,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_roots::{MakefileVariable, ReleaseRoots};
use backhopper_core::model::release_version::HostVersion;

fn file_pins(path: &str) -> PinSource {
    PinSource::ErlangMk {
        file_path: RelativePath::new(path).unwrap(),
    }
}

fn apps(names: &[&str]) -> ReleaseRoots {
    let mut names = names
        .iter()
        .map(|name| ApplicationName::new(*name).unwrap());
    ReleaseRoots::Apps(NonEmpty::new(names.next().unwrap(), names))
}

fn table(ra: &str) -> PinSource {
    let mut deps = BTreeMap::new();
    deps.insert(
        DependencyName::new("ra").unwrap(),
        DependencyVersion::new(ra).unwrap(),
    );
    PinSource::Table(BTreeMap::from([(
        "4.2.0".parse::<HostVersion>().unwrap(),
        deps,
    )]))
}

#[test]
fn the_digest_is_the_same_for_the_same_facts() {
    let first = host_facts_digest(&file_pins("rabbitmq-components.mk"), &apps(&["rabbit"]));
    let second = host_facts_digest(&file_pins("rabbitmq-components.mk"), &apps(&["rabbit"]));
    assert_eq!(first, second);
}

#[test]
fn the_pin_file_changes_the_digest() {
    let roots = apps(&["rabbit"]);
    assert_ne!(
        host_facts_digest(&file_pins("rabbitmq-components.mk"), &roots),
        host_facts_digest(&file_pins("deps.mk"), &roots)
    );
}

#[test]
fn a_pin_table_and_its_content_change_the_digest() {
    let roots = apps(&["rabbit"]);
    assert_ne!(
        host_facts_digest(&table("2.17.1"), &roots),
        host_facts_digest(&table("2.17.3"), &roots)
    );
    assert_ne!(
        host_facts_digest(&table("2.17.1"), &roots),
        host_facts_digest(&file_pins("rabbitmq-components.mk"), &roots)
    );
}

#[test]
fn the_order_of_the_root_apps_does_not_change_the_digest() {
    let pins = file_pins("rabbitmq-components.mk");
    assert_eq!(
        host_facts_digest(&pins, &apps(&["rabbit", "rabbit_common"])),
        host_facts_digest(&pins, &apps(&["rabbit_common", "rabbit"]))
    );
}

#[test]
fn literal_roots_and_a_variable_are_different_facts() {
    let pins = file_pins("rabbitmq-components.mk");
    let variable = MakefileVariable {
        file_path: RelativePath::new("plugins.mk").unwrap(),
        variable: MakeVariableName::new("PLUGINS").unwrap(),
    };
    let literal = host_facts_digest(&pins, &apps(&["rabbit"]));
    let from_variable = host_facts_digest(&pins, &ReleaseRoots::Variable(variable.clone()));
    let both = host_facts_digest(
        &pins,
        &ReleaseRoots::Both {
            apps: NonEmpty::new(ApplicationName::new("rabbit").unwrap(), []),
            variable,
        },
    );
    assert_ne!(literal, from_variable);
    assert_ne!(literal, both);
    assert_ne!(from_variable, both);
}
