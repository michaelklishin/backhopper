// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Properties of the target-tree preimage classifier: it never panics
//! on arbitrary input, never emits a drift and a collision for one
//! hunk, and always flags a constructed trailing add/add.

use proptest::prelude::*;

use backhopper_core::compat::classify_hunks_against_target;
use backhopper_core::compat::patch::Patch;
use backhopper_core::model::verdict::Reason;

fn reasons(patch: &str, target: &str) -> Vec<Reason> {
    let parsed = match Patch::parse(patch.as_bytes()) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let Some(file) = parsed.files.first() else {
        return Vec::new();
    };
    let Some(path) = file.new_path.as_deref() else {
        return Vec::new();
    };
    classify_hunks_against_target(path, &file.hunks, target)
}

proptest! {
    #[test]
    fn never_panics_on_arbitrary_patch_and_target(patch in ".{0,400}", target in ".{0,400}") {
        let _ = reasons(&patch, &target);
    }

    // A clean EOF append and a divergent one share a base; only the divergent target is a collision.
    #[test]
    fn a_trailing_add_over_divergent_content_is_flagged(
        ctx in "[a-z]{1,8}",
        added in "[a-z]{1,8}",
        target_tail in "[A-Z]{1,8}",
    ) {
        prop_assume!(added != target_tail);
        let patch = format!(
            "diff --git a/src/x.erl b/src/x.erl\n\
             --- a/src/x.erl\n+++ b/src/x.erl\n\
             @@ -1,1 +1,2 @@\n {ctx}\n+{added}\n"
        );
        let target = format!("{ctx}\n{target_tail}\n");
        let out = reasons(&patch, &target);
        prop_assert!(
            out.iter().any(|r| matches!(r, Reason::PostimageCollision { .. })),
            "expected a collision in {out:?}"
        );
    }

    // A hunk never carries both a drift note and a collision: the collision replaces the clean-shift signal.
    #[test]
    fn drift_and_collision_are_mutually_exclusive_per_hunk(patch in ".{0,300}", target in ".{0,300}") {
        let out = reasons(&patch, &target);
        let drifted = out.iter().filter(|r| matches!(r, Reason::PreimageDrifted { .. })).count();
        let collided = out.iter().filter(|r| matches!(r, Reason::PostimageCollision { .. })).count();
        // one hunk per generated patch at most, so the two never coexist
        prop_assert!(drifted == 0 || collided == 0);
    }
}
