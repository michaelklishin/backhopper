// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::collection::vec;
use proptest::prelude::any;
use proptest::proptest;

use backhopper_core::compat::patch::Patch;

proptest! {
    #[test]
    fn detector_does_not_panic_on_arbitrary_diff(bytes in vec(any::<u8>(), 0..2048)) {
        if let Ok(p) = Patch::parse(&bytes) {
            let _ = p.analyze().is_only_test_visibility_change();
        }
    }

    #[test]
    fn detector_does_not_panic_on_unified_diff_like_text(
        prefix in "diff --git a/[a-z_./]{1,32}\\.erl b/[a-z_./]{1,32}\\.erl\n--- a/[a-z_./]{1,32}\\.erl\n\\+\\+\\+ b/[a-z_./]{1,32}\\.erl\n@@ -1,5 \\+1,5 @@\n",
        body in "([\\x20-\\x7e]{0,128}\\n){0,20}",
    ) {
        let text = format!("{prefix}{body}");
        if let Ok(p) = Patch::parse(text.as_bytes()) {
            let _ = p.analyze().is_only_test_visibility_change();
        }
    }
}
