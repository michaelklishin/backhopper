// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use backhopper_core::compat::target_tree::normalise;

#[test]
fn normalise_swaps_backslashes_to_forward() {
    let p = PathBuf::from(r"deps\rabbit\src\foo.erl");
    let out = normalise(&p).expect("safe");
    assert_eq!(out.to_string_lossy(), "deps/rabbit/src/foo.erl");
}

#[test]
fn normalise_strips_leading_dot_slash() {
    let p = PathBuf::from("./deps/rabbit/src/foo.erl");
    let out = normalise(&p).expect("safe");
    assert_eq!(out.to_string_lossy(), "deps/rabbit/src/foo.erl");
}

#[test]
fn normalise_rejects_dotdot_segments() {
    let p = PathBuf::from("deps/../etc/passwd");
    assert!(normalise(&p).is_none());
}

#[test]
fn normalise_passes_through_clean_paths() {
    let p = Path::new("deps/rabbit/src/foo.erl");
    let out = normalise(p).expect("clean");
    assert_eq!(out, PathBuf::from("deps/rabbit/src/foo.erl"));
}

#[test]
fn normalise_strips_repeated_dot_slash() {
    let p = PathBuf::from(".//deps/x.erl");
    let out = normalise(&p).expect("safe");
    assert_eq!(out.to_string_lossy(), "/deps/x.erl");
}
