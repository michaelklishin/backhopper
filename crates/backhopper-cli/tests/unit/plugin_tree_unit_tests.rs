// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use backhopper_cli::commands::plugin_tree::{PluginTreeError, walk};
use backhopper_core::jigsaw::plugin_set::PluginSources;
use backhopper_core::model::names::ApplicationName;

fn app(name: &str) -> ApplicationName {
    ApplicationName::new(name).unwrap()
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

fn erlang_mk_app(root: &Path, name: &str, deps: &str) {
    write(
        root,
        "Makefile",
        &format!("PROJECT = {name}\nDEPS = {deps}\n"),
    );
    write(
        root,
        &format!("src/{name}.erl"),
        &format!("-module({name}).\n"),
    );
}

fn read(dirs: &[&Path], host: &[&str]) -> Result<PluginSources, PluginTreeError> {
    let dirs: Vec<PathBuf> = dirs.iter().map(|d| d.to_path_buf()).collect();
    let host: BTreeSet<ApplicationName> = host.iter().map(|a| app(a)).collect();
    walk(&dirs, &host)
}

fn paths(sources: &PluginSources, name: &str) -> Vec<String> {
    sources
        .apps
        .iter()
        .find(|a| a.manifest.name().as_str() == name)
        .unwrap()
        .files
        .iter()
        .map(|f| f.path.to_string())
        .collect()
}

#[test]
fn an_erlang_mk_app_is_read_from_its_makefile() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbitmq_shovel_extra", "rabbit_common ra");
    let sources = read(&[dir.path()], &[]).unwrap();
    assert_eq!(sources.apps.len(), 1);
    let manifest = &sources.apps[0].manifest;
    assert_eq!(manifest.name().as_str(), "rabbitmq_shovel_extra");
    assert_eq!(manifest.requires().len(), 2);
    assert_eq!(sources.apps[0].manifest_path.as_str(), "Makefile");
}

#[test]
fn an_app_src_wins_over_a_makefile() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbitmq_shovel_extra", "rabbit_common");
    write(
        dir.path(),
        "src/rabbitmq_shovel_extra.app.src",
        "{application, rabbitmq_shovel_extra, [{vsn, \"1\"}, {applications, [kernel, stdlib]}]}.\n",
    );
    let sources = read(&[dir.path()], &[]).unwrap();
    let manifest = &sources.apps[0].manifest;
    assert!(manifest.requires().contains(&app("kernel")));
    assert!(!manifest.requires().contains(&app("rabbit_common")));
    assert_eq!(
        sources.apps[0].manifest_path.as_str(),
        "src/rabbitmq_shovel_extra.app.src"
    );
}

#[test]
fn only_production_files_are_read() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbitmq_shovel_extra", "");
    write(dir.path(), "src/helper/util.erl", "-module(util).\n");
    write(dir.path(), "include/shovel.hrl", "-define(A, 1).\n");
    write(
        dir.path(),
        "priv/schema/shovel.schema",
        "{mapping, \"a\", \"b\", []}.\n",
    );
    write(
        dir.path(),
        "priv/schema/shovel.partial",
        "{mapping, \"c\", \"d\", []}.\n",
    );
    write(dir.path(), "priv/schema/shovel.snippets", "ignored\n");
    write(dir.path(), "priv/other/data.erl", "ignored\n");
    write(
        dir.path(),
        "test/shovel_SUITE.erl",
        "-module(shovel_SUITE).\n",
    );
    write(
        dir.path(),
        "_build/default/lib/x/src/x.erl",
        "-module(x).\n",
    );
    let sources = read(&[dir.path()], &[]).unwrap();
    assert_eq!(
        paths(&sources, "rabbitmq_shovel_extra"),
        [
            "include/shovel.hrl",
            "priv/schema/shovel.partial",
            "priv/schema/shovel.schema",
            "src/helper/util.erl",
            "src/rabbitmq_shovel_extra.erl",
        ]
    );
}

#[test]
fn a_directory_of_apps_gives_each_of_them() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(
        &dir.path().join("plugins/rabbitmq_shovel_extra"),
        "rabbitmq_shovel_extra",
        "",
    );
    erlang_mk_app(
        &dir.path().join("plugins/rabbitmq_shovel_core"),
        "rabbitmq_shovel_core",
        "",
    );
    let sources = read(&[&dir.path().join("plugins")], &[]).unwrap();
    let names: Vec<&str> = sources
        .apps
        .iter()
        .map(|a| a.manifest.name().as_str())
        .collect();
    assert_eq!(names, ["rabbitmq_shovel_core", "rabbitmq_shovel_extra"]);
}

#[test]
fn host_apps_in_deps_are_left_out_and_the_rest_are_bundled() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbitmq_shovel_extra", "rabbit_common thoas");
    erlang_mk_app(&dir.path().join("deps/rabbit_common"), "rabbit_common", "");
    erlang_mk_app(&dir.path().join("deps/rabbit"), "rabbit", "");
    erlang_mk_app(&dir.path().join("deps/thoas"), "thoas", "");
    let sources = read(&[dir.path()], &["rabbit", "rabbit_common"]).unwrap();
    let names: Vec<&str> = sources
        .apps
        .iter()
        .map(|a| a.manifest.name().as_str())
        .collect();
    assert_eq!(names, ["rabbitmq_shovel_extra", "thoas"]);
    assert_eq!(
        sources.host_provided,
        BTreeSet::from([app("rabbit"), app("rabbit_common")])
    );
}

#[test]
fn an_app_given_as_a_root_is_kept_even_when_the_host_has_one_of_the_name() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbit_common", "");
    let sources = read(&[dir.path()], &["rabbit_common"]).unwrap();
    assert_eq!(sources.apps.len(), 1);
    assert!(sources.host_provided.is_empty());
}

#[test]
fn a_manifest_whose_requirements_cannot_be_read_fails_naming_the_app() {
    let dir = TempDir::new().unwrap();
    erlang_mk_app(dir.path(), "rabbitmq_shovel_extra", "ranch $(EXTRA_DEPS)");
    let error = read(&[dir.path()], &[]).unwrap_err();
    assert!(matches!(
        &error,
        PluginTreeError::UnreadableManifest { app, .. } if app.as_str() == "rabbitmq_shovel_extra"
    ));
}

#[test]
fn a_directory_without_an_app_is_an_error() {
    let dir = TempDir::new().unwrap();
    write(dir.path(), "src/loose.erl", "-module(loose).\n");
    write(dir.path(), "Makefile", "all:\n\techo hi\n");
    assert!(matches!(
        read(&[dir.path()], &[]).unwrap_err(),
        PluginTreeError::NoApps { .. }
    ));
}

#[test]
fn a_missing_directory_is_an_io_error() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("nowhere");
    assert!(matches!(
        read(&[&missing], &[]).unwrap_err(),
        PluginTreeError::Io { .. }
    ));
}
