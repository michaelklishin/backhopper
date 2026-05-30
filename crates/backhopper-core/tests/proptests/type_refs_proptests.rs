// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::collection::vec;
use proptest::proptest;

use backhopper_core::compat::call_sites::{AttrCtxScanner, extract_type_refs_into};

proptest! {
    #[test]
    fn extract_type_refs_does_not_panic_on_any_text(s in ".*") {
        let mut out = Vec::new();
        extract_type_refs_into(&s, &mut out);
    }

    #[test]
    fn extract_type_refs_does_not_panic_on_lineful_text(text in "[^\\n]{0,256}(\\n[^\\n]{0,128}){0,8}") {
        let mut out = Vec::new();
        for line in text.lines() {
            extract_type_refs_into(line, &mut out);
        }
    }

    #[test]
    fn scanner_classify_does_not_panic_on_garbage_lines(lines in vec("[\\x20-\\x7e]{0,128}", 0..16)) {
        let mut s = AttrCtxScanner::new();
        for line in &lines {
            let _ = s.classify(line);
        }
    }
}
