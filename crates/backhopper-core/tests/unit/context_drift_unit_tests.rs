// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use time::OffsetDateTime;

use backhopper_core::compat::patch::{EvaluationFiles, Patch};
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};
use backhopper_core::model::verdict::Reason;

const ORIGINAL: &str = "-module(demo).\n-export([greet/1]).\ngreet(Name) -> Name.\n";
const PATCH: &str = "\
diff --git a/src/demo.erl b/src/demo.erl
--- a/src/demo.erl
+++ b/src/demo.erl
@@ -1,3 +1,4 @@
 -module(demo).
 -export([greet/1]).
+example() -> demo:greet(<<\"x\">>).
 greet(Name) -> Name.
";

fn snapshot() -> Snapshot<state::Canonical> {
    let header = SnapshotHeader {
        project: ProjectName::new("demo").unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
    };
    let mut m = Module::new(ModuleName::new("demo").unwrap());
    m.visibility = Visibility::Public;
    m.exports.push(FunArity {
        name: FunctionName::new("greet").unwrap(),
        arity: Arity::new(1),
    });
    Snapshot::from_extracted(header, vec![m], vec![]).into_canonical()
}

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("demo").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

#[test]
fn file_absent_when_path_missing_at_pin() {
    // a snapshot WITHOUT the `demo` module: a missing demo.erl is a
    // genuine FileAbsent, not a relocation
    let header = SnapshotHeader {
        project: ProjectName::new("demo").unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
    };
    let empty_snap = Snapshot::from_extracted(header, vec![], vec![]).into_canonical();
    let files = EvaluationFiles::new().with(PathBuf::from("src/demo.erl"), None);
    let series = Patch::parse(PATCH.as_bytes())
        .unwrap()
        .analyze()
        .against_series_with_files(&[(pin(), empty_snap, files)]);
    let r0 = &series.results[0];
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::FileAbsent { path } if path.ends_with("demo.erl")))
    );
}

#[test]
fn no_drift_when_context_matches() {
    let files = EvaluationFiles::new().with(
        PathBuf::from("src/demo.erl"),
        Some(ORIGINAL.as_bytes().to_vec()),
    );
    let series = Patch::parse(PATCH.as_bytes())
        .unwrap()
        .analyze()
        .against_series_with_files(&[(pin(), snapshot(), files)]);
    let r0 = &series.results[0];
    assert!(
        !r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::ContextDrift { .. }))
    );
}

#[test]
fn preimage_missing_reported_when_target_diverges() {
    let drifted = "-module(demo).\n-export([greet/2]).\ngreet(Name, _) -> Name.\n";
    let files = EvaluationFiles::new().with(
        PathBuf::from("src/demo.erl"),
        Some(drifted.as_bytes().to_vec()),
    );
    let series = Patch::parse(PATCH.as_bytes())
        .unwrap()
        .analyze()
        .against_series_with_files(&[(pin(), snapshot(), files)]);
    let r0 = &series.results[0];
    assert!(
        r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::PreimageMissing { hunk_index: 0, .. }))
    );
}

#[test]
fn pin_files_unset_for_path_yields_no_file_check() {
    let files = EvaluationFiles::new();
    let series = Patch::parse(PATCH.as_bytes())
        .unwrap()
        .analyze()
        .against_series_with_files(&[(pin(), snapshot(), files)]);
    let r0 = &series.results[0];
    assert!(
        !r0.verdict
            .reasons()
            .iter()
            .any(|r| matches!(r, Reason::FileAbsent { .. } | Reason::ContextDrift { .. }))
    );
}
