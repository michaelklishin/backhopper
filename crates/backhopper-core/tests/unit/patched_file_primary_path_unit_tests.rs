// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::{Path, PathBuf};

use backhopper_core::compat::patch::{Language, PatchedFile};

fn file(old: Option<&str>, new: Option<&str>) -> PatchedFile {
    PatchedFile {
        old_path: old.map(PathBuf::from),
        new_path: new.map(PathBuf::from),
        language: Language::Erlang,
        binary: false,
        hunks: Vec::new(),
    }
}

#[test]
fn primary_path_prefers_the_new_path() {
    let f = file(
        Some("deps/ra/src/ra.erl"),
        Some("deps/ra/src/ra_server.erl"),
    );
    assert_eq!(
        f.primary_path(),
        Some(Path::new("deps/ra/src/ra_server.erl"))
    );
}

#[test]
fn primary_path_falls_back_to_the_old_path_on_delete() {
    let f = file(Some("deps/ra/src/ra.erl"), None);
    assert_eq!(f.primary_path(), Some(Path::new("deps/ra/src/ra.erl")));
}

#[test]
fn primary_path_is_none_when_both_paths_absent() {
    let f = file(None, None);
    assert_eq!(f.primary_path(), None);
}
