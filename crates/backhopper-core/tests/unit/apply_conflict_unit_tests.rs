// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Reason::apply_conflict`: the risky apply outcomes a predictor reads,
//! and their severity order for dedup.

use std::path::{Path, PathBuf};

use backhopper_core::model::verdict::{ApplyConflictKind, Reason};

fn path(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn the_three_risky_reasons_map_with_their_path() {
    let cases = [
        (
            Reason::PostimageCollision {
                path: path("src/x.erl"),
                hunk_index: 0,
            },
            ApplyConflictKind::PostimageCollision,
        ),
        (
            Reason::PreimageMissing {
                path: path("src/x.erl"),
                hunk_index: 0,
                preimage_excerpt: String::new(),
            },
            ApplyConflictKind::PreimageMissing,
        ),
        (
            Reason::FileAbsent {
                path: path("src/x.erl"),
            },
            ApplyConflictKind::FileAbsent,
        ),
    ];
    for (reason, expected) in cases {
        let (p, kind) = reason.apply_conflict().expect("risky reason");
        assert_eq!(p, Path::new("src/x.erl"));
        assert_eq!(kind, expected);
    }
}

// A drifted preimage applies cleanly, so it is not a conflict.
#[test]
fn a_drifted_preimage_is_not_a_conflict() {
    let drifted = Reason::PreimageDrifted {
        path: path("src/x.erl"),
        hunk_index: 0,
        line_delta: 3,
    };
    assert!(drifted.apply_conflict().is_none());
}

#[test]
fn a_non_apply_reason_is_not_a_conflict() {
    let absent = Reason::UntrackedModuleMissing {
        module: "rabbit_db".parse().unwrap(),
    };
    assert!(absent.apply_conflict().is_none());
}

// Declaration order is severity: a missing file outranks a missing
// preimage, which outranks an edge collision. Dedup keeps the max.
#[test]
fn severity_order_is_file_absent_then_missing_then_collision() {
    assert!(ApplyConflictKind::FileAbsent > ApplyConflictKind::PreimageMissing);
    assert!(ApplyConflictKind::PreimageMissing > ApplyConflictKind::PostimageCollision);
    let max = [
        ApplyConflictKind::PostimageCollision,
        ApplyConflictKind::FileAbsent,
        ApplyConflictKind::PreimageMissing,
    ]
    .into_iter()
    .max()
    .unwrap();
    assert_eq!(max, ApplyConflictKind::FileAbsent);
}
