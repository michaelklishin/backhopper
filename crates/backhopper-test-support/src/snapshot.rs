// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Builders for the snapshot model: headers, modules, canonical
//! snapshots, and pins.

use time::OffsetDateTime;

use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{FunArity, Module, Snapshot, SnapshotHeader, state};

/// A `SnapshotHeader` with neutral defaults: no branch, an all-zero
/// commit, a single `src` scan path, the unix epoch, and no dep pins.
pub fn snapshot_header(project: &str, tag: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new(tag).unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src".into()],
        apps_scanned: Vec::new(),
        generated_by: "test".into(),
        generated_at: OffsetDateTime::UNIX_EPOCH,
        extractor_version: String::new(),
        dep_pins: Vec::new(),
    }
}

/// A public module with no exports.
pub fn module(name: &str) -> Module {
    Module::new(ModuleName::new(name).unwrap())
}

/// A public module with the given `name/arity` exports.
pub fn module_with(name: &str, exports: &[(&str, u8)]) -> Module {
    let mut m = module(name);
    for (f, a) in exports {
        m.exports.push(FunArity {
            name: FunctionName::new(*f).unwrap(),
            arity: Arity::new(*a),
        });
    }
    m
}

/// A canonical snapshot from a header and modules, with no `.hrl` files.
pub fn canonical_snapshot(
    header: SnapshotHeader,
    modules: Vec<Module>,
) -> Snapshot<state::Canonical> {
    Snapshot::from_extracted(header, modules, Vec::new()).into_canonical()
}

/// A resolved `Pin` for a project at a tag.
pub fn pin(project: &str, tag: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new(tag).unwrap(),
    )
}
