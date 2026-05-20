// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use tempfile::TempDir;

use backhopper_core::app_src::{DiscoveryWarning, discover};
use backhopper_core::model::names::{ApplicationName, ModuleName};

#[test]
fn discover_walks_rabbitmq_style_layout() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    let rabbit = root.join("deps/rabbit/src");
    fs::create_dir_all(&rabbit).unwrap();
    fs::write(
        rabbit.join("rabbit.app.src"),
        "{application, rabbit, [{vsn, \"4.2.0\"}, {modules, [rabbit_amqqueue, rabbit_db]}, {applications, [kernel, stdlib, ranch]}]}.",
    )
    .unwrap();

    let mgmt = root.join("deps/rabbitmq_management/src");
    fs::create_dir_all(&mgmt).unwrap();
    fs::write(
        mgmt.join("rabbitmq_management.app.src"),
        "{application, rabbitmq_management, [{vsn, \"4.2.0\"}, {modules, [rabbit_mgmt_app]}, {applications, [rabbit]}]}.",
    )
    .unwrap();

    let out = discover(root);
    assert_eq!(out.warnings, vec![]);
    assert_eq!(out.specs.len(), 2);

    let names: Vec<&str> = out.specs.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"rabbit"));
    assert!(names.contains(&"rabbitmq_management"));

    let rabbit_spec = out
        .specs
        .iter()
        .find(|s| s.name == ApplicationName::new("rabbit").unwrap())
        .unwrap();
    assert_eq!(rabbit_spec.modules.len(), 2);
    assert!(
        rabbit_spec
            .modules
            .contains(&ModuleName::new("rabbit_amqqueue").unwrap())
    );
    assert_eq!(rabbit_spec.applications.len(), 3);
}

#[test]
fn discover_skips_build_output_directories() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("real.app.src"),
        "{application, real, [{modules, []}]}.",
    )
    .unwrap();

    // Build outputs and vendored deps that must be skipped.
    for dir_to_skip in &[
        "_build",
        "_rel",
        "node_modules",
        ".direnv",
        "target",
        "logs",
    ] {
        let nested = root.join(dir_to_skip).join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            nested.join("fake.app.src"),
            "{application, fake, [{modules, []}]}.",
        )
        .unwrap();
    }

    let out = discover(root);
    let names: Vec<&str> = out.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["real"]);
}

#[test]
fn discover_warns_on_parse_failures_but_keeps_walking() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/broken.app.src"), "this is not valid").unwrap();
    fs::write(
        root.join("src/good.app.src"),
        "{application, good, [{modules, []}]}.",
    )
    .unwrap();

    let out = discover(root);
    assert_eq!(out.specs.len(), 1);
    assert_eq!(out.specs[0].name.as_str(), "good");
    assert_eq!(out.warnings.len(), 1);
    assert!(matches!(
        out.warnings[0],
        DiscoveryWarning::ParseFailed { .. }
    ));
}

#[test]
fn discover_accepts_compiled_dot_app_files_too() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("ebin")).unwrap();
    fs::write(
        root.join("ebin/compiled.app"),
        "{application, compiled, [{modules, [a, b]}]}.",
    )
    .unwrap();

    let out = discover(root);
    assert_eq!(out.specs.len(), 1);
    assert_eq!(out.specs[0].name.as_str(), "compiled");
    assert_eq!(out.specs[0].modules.len(), 2);
}

#[test]
fn discover_results_are_sorted_by_path() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    for app in &["zebra", "antelope", "lion"] {
        let p = root.join(format!("deps/{app}/src"));
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join(format!("{app}.app.src")),
            format!("{{application, {app}, [{{modules, []}}]}}."),
        )
        .unwrap();
    }
    let out = discover(root);
    let names: Vec<&str> = out.specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["antelope", "lion", "zebra"]);
}

#[test]
fn discover_empty_root_yields_empty_output() {
    let dir = TempDir::new().expect("tempdir");
    let out = discover(dir.path());
    assert!(out.specs.is_empty());
    assert!(out.warnings.is_empty());
}

#[test]
fn discover_nonexistent_root_yields_empty_output() {
    let dir = TempDir::new().expect("tempdir");
    let missing = dir.path().join("does/not/exist");
    let out = discover(&missing);
    assert!(out.specs.is_empty());
    assert!(out.warnings.is_empty());
}