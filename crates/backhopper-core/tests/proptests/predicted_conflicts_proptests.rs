// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `BatchResult::predicted_conflicts` over arbitrary risky reasons: one
//! entry per path, each at the highest severity seen for that path.

use std::collections::BTreeMap;

use proptest::prelude::*;

use backhopper_core::model::batch::BatchResult;
use backhopper_core::model::names::{CommitSha, ProjectName, SeriesName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    ApplyConflictKind, Diagnostics, PatchFacts, PinVerdict, Reason, SeriesVerdict, Verdict,
};

fn reason(path_index: usize, kind: ApplyConflictKind) -> Reason {
    let path = format!("src/f{path_index}.erl").into();
    match kind {
        ApplyConflictKind::PostimageCollision => Reason::PostimageCollision {
            path,
            hunk_index: 0,
        },
        ApplyConflictKind::PreimageMissing => Reason::PreimageMissing {
            path,
            hunk_index: 0,
            preimage_excerpt: String::new(),
        },
        ApplyConflictKind::FileAbsent => Reason::FileAbsent { path },
    }
}

fn kind(index: usize) -> ApplyConflictKind {
    match index {
        0 => ApplyConflictKind::PostimageCollision,
        1 => ApplyConflictKind::PreimageMissing,
        _ => ApplyConflictKind::FileAbsent,
    }
}

fn row(reasons: Vec<Reason>) -> BatchResult {
    let pin = Pin::new(
        ProjectName::new("rabbit").unwrap(),
        TagName::new("v4.1.0").unwrap(),
    );
    let verdict = SeriesVerdict::from_results(vec![PinVerdict::new(
        pin,
        Verdict::RequiresAdaptation { reasons },
    )]);
    BatchResult {
        commit: CommitSha::new("a".repeat(40)).unwrap(),
        series: SeriesName::new("v4.1.x").unwrap(),
        verdict,
        diagnostics: Diagnostics::default(),
        patch_facts: PatchFacts::default(),
        touched_paths: Vec::new(),
        pr_commits: None,
        parent_count: None,
        verdict_fingerprint: None,
        apply: None,
        target_findings: None,
    }
}

proptest! {
    #[test]
    fn one_entry_per_path_at_the_max_severity(
        picks in proptest::collection::vec((0usize..4, 0usize..3), 0..24)
    ) {
        let mut expected: BTreeMap<usize, ApplyConflictKind> = BTreeMap::new();
        let mut reasons = Vec::new();
        for (path_index, kind_index) in &picks {
            let k = kind(*kind_index);
            reasons.push(reason(*path_index, k));
            expected
                .entry(*path_index)
                .and_modify(|seen| *seen = (*seen).max(k))
                .or_insert(k);
        }
        let conflicts = row(reasons).predicted_conflicts();
        prop_assert_eq!(conflicts.len(), expected.len());
        for (path_index, kind) in expected {
            let want = format!("src/f{path_index}.erl");
            let got = conflicts.iter().find(|c| c.path.to_str() == Some(want.as_str())).unwrap();
            prop_assert_eq!(got.kind, kind);
        }
    }
}
