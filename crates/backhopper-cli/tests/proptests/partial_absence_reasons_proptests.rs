// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants for `partial_absence_reasons`: empty when no path is on
//! the target, otherwise one `TargetPathAbsent` per missing path.

use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_cli::commands::target_repo::{TouchedPathSummary, partial_absence_reasons};
use backhopper_core::model::verdict::Reason;

fn summary_strategy() -> impl Strategy<Value = TouchedPathSummary> {
    (proptest::collection::vec(0usize..50, 0..10), 0usize..5).prop_map(|(idxs, on_target)| {
        TouchedPathSummary {
            renames: Vec::new(),
            missing: idxs
                .into_iter()
                .map(|i| PathBuf::from(format!("deps/demo/src/f{i}.erl")))
                .collect(),
            on_target,
        }
    })
}

proptest! {
    #[test]
    fn empty_unless_some_path_is_on_target(summary in summary_strategy()) {
        let reasons = partial_absence_reasons(&summary);
        if summary.on_target == 0 {
            prop_assert!(reasons.is_empty());
        } else {
            prop_assert_eq!(reasons.len(), summary.missing.len());
            for r in &reasons {
                let Reason::TargetPathAbsent { path } = r else {
                    return Err(TestCaseError::fail(format!("unexpected reason {r:?}")));
                };
                prop_assert!(
                    summary.missing.iter().any(|p| p.to_string_lossy() == path.as_str())
                );
            }
        }
    }
}
