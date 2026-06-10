// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_git::GitError;
#[test]
fn ambiguous_sha_display_uses_truncated_at_count() {
    let err = GitError::AmbiguousSha {
        prefix: "abc1234".into(),
        candidates: vec!["abc12340000000000000000000000000000aaaaa".into()],
        truncated_at: 12,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("12 objects"),
        "expected truncated_at count, got: {msg}"
    );
}

#[test]
fn ambiguous_sha_display_uses_singular_form_for_one_match() {
    let err = GitError::AmbiguousSha {
        prefix: "abc1234".into(),
        candidates: vec!["abc12340000000000000000000000000000aaaaa".into()],
        truncated_at: 1,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("1 object,") || msg.ends_with("1 object"),
        "expected singular form, got: {msg}"
    );
}
