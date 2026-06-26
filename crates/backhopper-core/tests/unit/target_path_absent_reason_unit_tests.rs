// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Reason::TargetPathAbsent` classification: the partial-absence
//! advisory promotes to `RequiresAdaptation`, never `Incompatible`,
//! and carries no symbol or apply-conflict class.

use backhopper_core::model::names::RelativePath;
use backhopper_core::model::verdict::{Reason, Verdict};

fn target_path_absent() -> Reason {
    Reason::TargetPathAbsent {
        path: RelativePath::new("deps/rabbit/src/rabbit_stream_super_stream_mgmt.erl").unwrap(),
    }
}

#[test]
fn is_non_blocking() {
    assert!(!target_path_absent().is_blocking());
}

#[test]
fn is_path_scoped() {
    assert!(target_path_absent().is_path_scoped());
}

#[test]
fn has_no_resolver_class() {
    assert_eq!(target_path_absent().resolver_class(), None);
}

// Not in the apply-conflict set the divergence predictor reads.
#[test]
fn is_not_an_apply_conflict() {
    assert_eq!(target_path_absent().apply_conflict(), None);
}

#[test]
fn alone_yields_requires_adaptation() {
    let verdict = Verdict::from_reasons(vec![target_path_absent()]);
    assert!(matches!(verdict, Verdict::RequiresAdaptation { .. }));
}

#[test]
fn serializes_with_snake_case_kind_and_path() {
    let json = serde_json::to_value(target_path_absent()).unwrap();
    assert_eq!(json["kind"], "target_path_absent");
    assert_eq!(
        json["path"],
        "deps/rabbit/src/rabbit_stream_super_stream_mgmt.erl"
    );
}
