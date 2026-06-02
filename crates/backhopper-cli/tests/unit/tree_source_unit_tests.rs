// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;

use backhopper_cli::commands::tree_source::{SKIP_DIRS, collect_erlang_files};
use tempfile::TempDir;

#[test]
fn picks_erl_and_hrl_only() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.erl"), b"-module(a).").unwrap();
    fs::write(dir.path().join("b.hrl"), b"%% header").unwrap();
    fs::write(dir.path().join("c.txt"), b"not erlang").unwrap();
    fs::write(dir.path().join("d.beam"), b"\x00").unwrap();

    let files = collect_erlang_files(dir.path()).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.erl".to_owned(), "b.hrl".to_owned()]);
}

#[test]
fn results_are_sorted_by_path() {
    let dir = TempDir::new().unwrap();
    for name in ["z.erl", "a.erl", "m.erl"] {
        fs::write(dir.path().join(name), b"").unwrap();
    }
    let files = collect_erlang_files(dir.path()).unwrap();
    let names: Vec<String> = files
        .iter()
        .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["a.erl".to_owned(), "m.erl".to_owned(), "z.erl".to_owned()]
    );
}

#[test]
fn skip_dirs_are_not_descended() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("top.erl"), b"").unwrap();
    for skip in SKIP_DIRS {
        let inner = dir.path().join(skip);
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("hidden.erl"), b"hidden").unwrap();
    }
    let files = collect_erlang_files(dir.path()).unwrap();
    assert_eq!(files.len(), 1, "only top.erl should be collected");
    assert_eq!(files[0].0.file_name().unwrap().to_string_lossy(), "top.erl");
}

#[test]
fn recurses_into_non_skipped_subdirs() {
    let dir = TempDir::new().unwrap();
    let nested = dir.path().join("lib/rabbit/src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("rabbit_app.erl"), b"-module(rabbit_app).").unwrap();
    let files = collect_erlang_files(dir.path()).unwrap();
    assert_eq!(files.len(), 1);
    assert!(files[0].0.ends_with("lib/rabbit/src/rabbit_app.erl"));
}

#[test]
fn reads_file_bytes_verbatim() {
    let dir = TempDir::new().unwrap();
    let body = b"-module(x).\n-export([f/0]).\nf() -> ok.\n";
    fs::write(dir.path().join("x.erl"), body).unwrap();
    let files = collect_erlang_files(dir.path()).unwrap();
    assert_eq!(files[0].1, body);
}

#[test]
fn missing_directory_returns_io_error() {
    let dir = TempDir::new().unwrap();
    let absent = dir.path().join("does-not-exist");
    let err = collect_erlang_files(&absent).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
