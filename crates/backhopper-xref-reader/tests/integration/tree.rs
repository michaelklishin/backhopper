// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_xref_reader::{ProjectLayout, SourceReader};

#[test]
fn read_tree_processes_two_modules_sorted_by_name() {
    let reader = SourceReader::new();
    let files = vec![
        (
            PathBuf::from("z.erl"),
            "-module(zebra).\n-export([go/0]).\ngo() -> ok.\n"
                .as_bytes()
                .to_vec(),
        ),
        (
            PathBuf::from("a.erl"),
            "-module(alpha).\n-export([go/0]).\ngo() -> ok.\n"
                .as_bytes()
                .to_vec(),
        ),
    ];
    let out = reader.read_tree(files).expect("read tree ok");
    let names: Vec<&str> = out.modules.iter().map(|m| m.module.as_str()).collect();
    assert_eq!(names, vec!["alpha", "zebra"]);
}

#[test]
fn read_tree_skips_non_erl_extensions() {
    let reader = SourceReader::new();
    let files = vec![
        (
            PathBuf::from("a.app.src"),
            b"{application, a, []}.\n".to_vec(),
        ),
        (
            PathBuf::from("a.erl"),
            "-module(a).\n-export([go/0]).\ngo() -> ok.\n"
                .as_bytes()
                .to_vec(),
        ),
    ];
    let out = reader.read_tree(files).expect("read tree ok");
    assert_eq!(out.modules.len(), 1);
}

#[test]
fn read_tree_with_layout_assigns_application() {
    let reader = SourceReader::with_layout(ProjectLayout::rabbitmq_main());
    let files = vec![(
        PathBuf::from("deps/rabbit/src/rabbit_x.erl"),
        "-module(rabbit_x).\n-export([go/0]).\ngo() -> ok.\n"
            .as_bytes()
            .to_vec(),
    )];
    let out = reader.read_tree(files).expect("read tree ok");
    let app = out.modules[0]
        .application
        .assigned()
        .expect("expected app assignment")
        .as_str();
    assert_eq!(app, "rabbit");
}

#[test]
fn read_tree_assigns_distinct_path_ids_per_file() {
    let reader = SourceReader::new();
    let files = vec![
        (
            PathBuf::from("a.erl"),
            "-module(a).\n-export([f/0]).\nf() -> ok.\n"
                .as_bytes()
                .to_vec(),
        ),
        (
            PathBuf::from("b.erl"),
            "-module(b).\n-export([f/0]).\nf() -> a:f().\n"
                .as_bytes()
                .to_vec(),
        ),
    ];
    let out = reader.read_tree(files).expect("read tree ok");
    assert_eq!(out.paths.len(), 2);
    let resolved_a = out
        .paths
        .get(out.modules[0].definitions.values().next().unwrap().path);
    let resolved_b = out
        .paths
        .get(out.modules[1].definitions.values().next().unwrap().path);
    assert_ne!(
        resolved_a, resolved_b,
        "two files should resolve to different paths"
    );
    assert!(
        resolved_a.is_some() && resolved_b.is_some(),
        "both ids resolve"
    );
}

#[test]
fn read_tree_preserves_warnings_from_each_file() {
    let reader = SourceReader::new();
    let files = vec![
        (PathBuf::from("empty.erl"), Vec::new()),
        (
            PathBuf::from("ok.erl"),
            "-module(ok_mod).\n-export([f/0]).\nf() -> ok.\n"
                .as_bytes()
                .to_vec(),
        ),
    ];
    let out = reader.read_tree(files).expect("read tree ok");
    assert_eq!(out.modules.len(), 1);
    assert!(
        out.warnings
            .iter()
            .any(|w| matches!(w, backhopper_xref_reader::ReadWarning::EmptyFile { .. }))
    );
}
