// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backhopper_core::compat::patch::Patch;
use backhopper_core::compat::source_macros::{FileMap, build_macro_table};
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::symbol::SymbolKind;

fn analyze_with_files(patch_bytes: &[u8], files: &FileMap) -> Vec<String> {
    let parsed = Patch::parse(patch_bytes).expect("parse");
    let mut per_path: BTreeMap<PathBuf, MacroTable> = BTreeMap::new();
    for file in &parsed.files {
        let Some(path) = file.new_path.clone().or_else(|| file.old_path.clone()) else {
            continue;
        };
        if let Some(body) = files.get(&path) {
            per_path.insert(path.clone(), build_macro_table(body, &path, files));
        }
    }
    let analyzed = parsed.analyze_with_macros(&per_path);
    analyzed
        .referenced()
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn macro_defined_in_same_file_resolves_in_patch() {
    let patch = b"\
diff --git a/m.erl b/m.erl
--- a/m.erl
+++ b/m.erl
@@ -1,3 +1,4 @@
 -module(m).
 -define(SERVER, my_module).
 -export([go/0]).
+go() -> ?SERVER:start(1, 2).
";
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("m.erl"),
        "-module(m).\n-define(SERVER, my_module).\n-export([go/0]).\ngo() -> ?SERVER:start(1, 2).\n".into(),
    );
    let calls = analyze_with_files(patch, &files);
    assert!(calls.iter().any(|c| c == "my_module:start/2"), "{calls:?}");
}

#[test]
fn macro_defined_in_included_header_resolves_in_patch() {
    let patch = b"\
diff --git a/deps/rabbit/src/m.erl b/deps/rabbit/src/m.erl
--- a/deps/rabbit/src/m.erl
+++ b/deps/rabbit/src/m.erl
@@ -3,2 +3,3 @@
 -export([go/0]).
+go() -> ?SERVER:start(1).
";
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("deps/rabbit/src/m.erl"),
        "-module(m).\n-include(\"shared.hrl\").\n-export([go/0]).\ngo() -> ?SERVER:start(1).\n"
            .into(),
    );
    files.insert(
        PathBuf::from("deps/rabbit/src/shared.hrl"),
        "-define(SERVER, my_module).\n".into(),
    );
    let calls = analyze_with_files(patch, &files);
    assert!(calls.iter().any(|c| c == "my_module:start/1"), "{calls:?}");
}

#[test]
fn apply_with_macro_module_resolves_via_source_context() {
    let patch = b"\
diff --git a/m.erl b/m.erl
--- a/m.erl
+++ b/m.erl
@@ -1,3 +1,4 @@
 -module(m).
 -define(SERVER, my_module).
 -export([go/0]).
+go() -> apply(?SERVER, init, [config]).
";
    let mut files = FileMap::new();
    files.insert(
        PathBuf::from("m.erl"),
        "-module(m).\n-define(SERVER, my_module).\n-export([go/0]).\ngo() -> apply(?SERVER, init, [config]).\n".into(),
    );
    let calls = analyze_with_files(patch, &files);
    assert!(calls.iter().any(|c| c == "my_module:init/1"), "{calls:?}");
}

#[test]
fn empty_file_map_falls_back_to_macro_free_behaviour() {
    let patch = b"\
diff --git a/m.erl b/m.erl
--- a/m.erl
+++ b/m.erl
@@ -1,1 +1,2 @@
 -module(m).
+go() -> spawn(worker, init, [cfg]).
";
    let calls = analyze_with_files(patch, &FileMap::new());
    assert!(
        calls.iter().any(|c| c == "worker:init/1"),
        "apply-family still resolves without source files: {calls:?}"
    );
}
