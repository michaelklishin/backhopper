// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::app_manifest::ManifestUnreadable;
use backhopper_core::model::names::{ApplicationName, MakeVariableName, RelativePath};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_roots::{MakefileVariable, ReleaseRoots, ReleaseRootsError};

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn plugins_variable() -> MakefileVariable {
    MakefileVariable {
        file_path: RelativePath::new("plugins.mk").unwrap(),
        variable: MakeVariableName::new("PLUGINS").unwrap(),
    }
}

fn read_plugins_mk(text: &'static str) -> impl Fn(&RelativePath) -> Option<String> {
    move |path| (path.as_str() == "plugins.mk").then(|| text.to_owned())
}

#[test]
fn literal_apps_are_the_roots() {
    let roots = ReleaseRoots::Apps(NonEmpty::new(app("rabbit"), []));
    let read = roots.read(|_| None).unwrap();
    assert_eq!(read.as_slice(), [app("rabbit")]);
}

#[test]
fn a_variable_supplies_the_roots() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    let read = roots
        .read(read_plugins_mk(
            "PLUGINS := rabbitmq_management rabbitmq_mqtt\n",
        ))
        .unwrap();
    assert_eq!(
        read.as_slice(),
        [app("rabbitmq_management"), app("rabbitmq_mqtt")]
    );
}

#[test]
fn both_sources_are_joined_with_the_literal_apps_first() {
    let roots = ReleaseRoots::Both {
        apps: NonEmpty::new(app("rabbit"), []),
        variable: plugins_variable(),
    };
    let read = roots
        .read(read_plugins_mk("PLUGINS = rabbitmq_stomp\n"))
        .unwrap();
    assert_eq!(read.as_slice(), [app("rabbit"), app("rabbitmq_stomp")]);
}

#[test]
fn a_missing_file_is_an_error() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    assert!(matches!(
        roots.read(|_| None),
        Err(ReleaseRootsError::FileMissing { .. })
    ));
}

#[test]
fn an_unexpandable_variable_fails_the_read() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    let error = roots
        .read(read_plugins_mk("PLUGINS = rabbitmq_management $(EXTRA)\n"))
        .unwrap_err();
    assert!(matches!(
        error,
        ReleaseRootsError::VariableUnreadable {
            reason: ManifestUnreadable::UnexpandedVariable(_),
            ..
        }
    ));
}

#[test]
fn an_unassigned_variable_fails_the_read() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    let error = roots.read(read_plugins_mk("OTHER = x\n")).unwrap_err();
    assert!(matches!(
        error,
        ReleaseRootsError::VariableUnreadable {
            reason: ManifestUnreadable::Missing,
            ..
        }
    ));
}

#[test]
fn an_empty_variable_alone_gives_no_roots() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    assert!(matches!(
        roots.read(read_plugins_mk("PLUGINS =\n")),
        Err(ReleaseRootsError::NoRoots { .. })
    ));
}

#[test]
fn a_word_that_is_not_an_app_name_fails_the_read() {
    let roots = ReleaseRoots::Variable(plugins_variable());
    assert!(matches!(
        roots.read(read_plugins_mk("PLUGINS = Rabbit-MQ\n")),
        Err(ReleaseRootsError::NotAnApp { .. })
    ));
}
