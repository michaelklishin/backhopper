// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::path::PathBuf;

use backhopper_core::jigsaw::plugin_set::{
    AppRole, PluginAppSources, PluginSet, PluginSetError, PluginSources, SourceFile,
};
use backhopper_core::model::app_manifest::{
    AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable,
};
use backhopper_core::model::names::{ApplicationName, MakeVariableName, RelativePath};

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn manifest(name: &str, requires: &[&str]) -> AppManifest {
    AppManifest {
        name: app(name),
        source: ManifestSource::ErlangMk,
        requires: ManifestDeps::Listed(requires.iter().map(|r| app(r)).collect()),
    }
}

fn sources(name: &str, requires: &[&str], files: &[(&str, &str)]) -> PluginAppSources {
    PluginAppSources {
        root: PathBuf::from(format!("/plugins/{name}")),
        manifest: manifest(name, requires).into_read().unwrap(),
        manifest_path: RelativePath::new("Makefile").unwrap(),
        files: files
            .iter()
            .map(|(path, text)| SourceFile {
                path: RelativePath::new(*path).unwrap(),
                bytes: text.as_bytes().to_vec(),
            })
            .collect(),
    }
}

fn set(apps: Vec<PluginAppSources>) -> Result<PluginSet, PluginSetError> {
    PluginSet::new(PluginSources {
        apps,
        host_provided: BTreeSet::new(),
    })
}

#[test]
fn a_manifest_with_a_readable_list_can_be_read() {
    let read = manifest("rabbitmq_shovel_extra", &["rabbit_common"])
        .into_read()
        .unwrap();
    assert_eq!(read.name().as_str(), "rabbitmq_shovel_extra");
    assert_eq!(read.requires().len(), 1);
}

#[test]
fn a_manifest_with_an_unreadable_list_cannot_be_read() {
    let unreadable = AppManifest {
        name: app("rabbitmq_shovel_extra"),
        source: ManifestSource::ErlangMk,
        requires: ManifestDeps::Unreadable(ManifestUnreadable::UnexpandedVariable(
            MakeVariableName::new("EXTRA_DEPS").unwrap(),
        )),
    };
    assert!(matches!(
        unreadable.into_read(),
        Err(ManifestUnreadable::UnexpandedVariable(_))
    ));
}

#[test]
fn two_apps_with_one_name_are_refused() {
    let error = set(vec![
        sources("rabbitmq_shovel_extra", &[], &[]),
        sources("rabbitmq_shovel_extra", &[], &[]),
    ])
    .unwrap_err();
    assert!(matches!(error, PluginSetError::DuplicateApp { .. }));
}

#[test]
fn two_modules_with_one_name_are_refused() {
    let error = set(vec![
        sources(
            "rabbitmq_shovel_extra",
            &[],
            &[(
                "src/rabbit_shovel_extra_worker.erl",
                "-module(rabbit_shovel_extra_worker).\n",
            )],
        ),
        sources(
            "rabbitmq_shovel_core",
            &[],
            &[("src/worker.erl", "-module(rabbit_shovel_extra_worker).\n")],
        ),
    ])
    .unwrap_err();
    assert!(
        matches!(&error, PluginSetError::DuplicateModule { module, .. } if module.as_str() == "rabbit_shovel_extra_worker")
    );
}

#[test]
fn an_app_another_app_requires_is_bundled_and_the_rest_are_plugins() {
    let plugins = set(vec![
        sources(
            "rabbitmq_shovel_extra",
            &["rabbitmq_shovel_core", "kernel"],
            &[],
        ),
        sources("rabbitmq_shovel_core", &[], &[]),
    ])
    .unwrap();
    assert_eq!(
        plugins.app(&app("rabbitmq_shovel_extra")).unwrap().role(),
        AppRole::Plugin
    );
    assert_eq!(
        plugins.app(&app("rabbitmq_shovel_core")).unwrap().role(),
        AppRole::Bundled
    );
}

#[test]
fn files_are_listed_with_their_app() {
    let plugins = set(vec![sources(
        "rabbitmq_shovel_extra",
        &[],
        &[("src/a.erl", "-module(a).\n"), ("include/a.hrl", "")],
    )])
    .unwrap();
    let files: Vec<String> = plugins
        .files()
        .map(|(id, _)| format!("{}:{}", id.app, id.path))
        .collect();
    assert_eq!(
        files,
        [
            "rabbitmq_shovel_extra:src/a.erl",
            "rabbitmq_shovel_extra:include/a.hrl"
        ]
    );
}
