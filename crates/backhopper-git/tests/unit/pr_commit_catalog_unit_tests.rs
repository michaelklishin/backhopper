// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::pr_commit::PrCommitKind;
use backhopper_git::classify;

const SUBSET_HELD: bool = true;
const SUBSET_FAILED: bool = false;

#[test]
fn substantive_is_default_when_subset_rule_fails() {
    assert_eq!(
        classify("Resolve conflicts", SUBSET_FAILED),
        PrCommitKind::Substantive
    );
    assert_eq!(
        classify("fixup! bla", SUBSET_FAILED),
        PrCommitKind::Substantive
    );
}

#[test]
fn substantive_when_subject_does_not_match_any_pattern() {
    assert_eq!(
        classify("Add new feature X", SUBSET_HELD),
        PrCommitKind::Substantive
    );
    assert_eq!(
        classify("Fix bug in module Y", SUBSET_HELD),
        PrCommitKind::Substantive
    );
}

#[test]
fn conflict_resolution_for_git_merge_default_subjects() {
    for s in [
        "Merge branch 'main' into mk-item-21",
        "Merge branch 'main' of github.com:rabbitmq/ra",
        "Merge branch 'main'",
        "Merge remote-tracking branch 'origin/main' into feature",
        "Merge remote-tracking branch 'origin/main'",
    ] {
        assert_eq!(
            classify(s, SUBSET_HELD),
            PrCommitKind::ConflictResolution,
            "expected ConflictResolution for {s:?}"
        );
    }
}

#[test]
fn conflict_resolution_for_bare_and_verb_form() {
    assert_eq!(
        classify("Conflicts", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(
        classify("Conflict", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(
        classify("Resolve conflicts", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(
        classify("Fix merge conflicts", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(
        classify("Address conflict", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
}

#[test]
fn conflict_resolution_for_descriptive_substring() {
    let s = "Fix another merge conflict resolution issue #13873 #14926";
    assert_eq!(classify(s, SUBSET_HELD), PrCommitKind::ConflictResolution);
}

#[test]
fn review_feedback_for_github_apply_suggestions() {
    assert_eq!(
        classify("Apply suggestions from code review", SUBSET_HELD),
        PrCommitKind::ReviewFeedback
    );
}

#[test]
fn review_feedback_for_address_feedback_patterns() {
    for s in [
        "Address review feedback",
        "Addressed requested changes",
        "Apply PR suggestions",
        "Address review feedback #14425",
        "Handle code review comments",
    ] {
        assert_eq!(
            classify(s, SUBSET_HELD),
            PrCommitKind::ReviewFeedback,
            "expected ReviewFeedback for {s:?}"
        );
    }
}

#[test]
fn fixup_for_git_and_manual_conventions() {
    assert_eq!(
        classify("fixup! Initial commit", SUBSET_HELD),
        PrCommitKind::Fixup
    );
    assert_eq!(
        classify("fixup: address feedback", SUBSET_HELD),
        PrCommitKind::Fixup
    );
}

#[test]
fn wip_or_cleanup_subjects() {
    assert_eq!(classify("WIP", SUBSET_HELD), PrCommitKind::WipOrCleanup);
    assert_eq!(
        classify("WIP: stage one", SUBSET_HELD),
        PrCommitKind::WipOrCleanup
    );
    assert_eq!(classify("Cleanup", SUBSET_HELD), PrCommitKind::WipOrCleanup);
    assert_eq!(classify("cleanup", SUBSET_HELD), PrCommitKind::WipOrCleanup);
}

#[test]
fn substantive_for_generic_fix_subjects() {
    for s in [
        "Fix bug in rabbit_fifo",
        "Fix race condition in stream coordinator",
        "Fix tests",
    ] {
        assert_eq!(
            classify(s, SUBSET_HELD),
            PrCommitKind::Substantive,
            "Fix-prefix subjects must NOT trigger bookkeeping: {s:?}"
        );
    }
}

#[test]
fn substantive_for_rebase_or_update_subjects() {
    for s in [
        "Rebase against main",
        "Update README.md",
        "Format with rustfmt",
    ] {
        assert_eq!(
            classify(s, SUBSET_HELD),
            PrCommitKind::Substantive,
            "{s:?} must classify as Substantive"
        );
    }
}

#[test]
fn case_insensitive_matching() {
    assert_eq!(
        classify("MERGE BRANCH 'MAIN' INTO X", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(
        classify("RESOLVE CONFLICTS", SUBSET_HELD),
        PrCommitKind::ConflictResolution
    );
    assert_eq!(classify("WIP", SUBSET_HELD), PrCommitKind::WipOrCleanup);
}
