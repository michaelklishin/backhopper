// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Property checks for the unified-diff parser. The parser is fed
//! arbitrary byte input and asserted to never panic, never read out of
//! bounds, and never lose well-formed hunks. The parser is `pub(crate)`,
//! so we exercise it through `Patch::parse`.

use proptest::prelude::*;

use backhopper_core::compat::patch::Patch;

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = Patch::parse(&bytes);
    }

    #[test]
    fn arbitrary_ascii_text_never_panics(text in "[\\x20-\\x7e\\n]{0,4096}") {
        let _ = Patch::parse(text.as_bytes());
    }

    #[test]
    fn minimal_one_hunk_diff_parses_back_to_one_file(
        old_path in "[a-z][a-z0-9_/]{0,12}\\.erl",
        new_path in "[a-z][a-z0-9_/]{0,12}\\.erl",
        old_start in 1usize..200,
        old_count in 1usize..50,
        new_start in 1usize..200,
        new_count in 1usize..50,
        body in "[a-z ]{0,40}"
    ) {
        let mut diff = String::new();
        diff.push_str(&format!("diff --git a/{} b/{}\n", old_path, new_path));
        diff.push_str(&format!("--- a/{}\n", old_path));
        diff.push_str(&format!("+++ b/{}\n", new_path));
        diff.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start, old_count, new_start, new_count,
        ));
        diff.push_str(&format!(" {}\n", body));
        let patch = Patch::parse(diff.as_bytes()).expect("well-formed diff should parse");
        prop_assert_eq!(patch.files.len(), 1);
        let f = &patch.files[0];
        prop_assert_eq!(f.hunks.len(), 1);
        prop_assert_eq!(f.hunks[0].old_start, old_start);
        prop_assert_eq!(f.hunks[0].new_start, new_start);
    }

    #[test]
    fn empty_input_parses_to_zero_files(noise in "[ \t\n]{0,32}") {
        let patch = Patch::parse(noise.as_bytes()).expect("blank input is valid");
        prop_assert!(patch.files.is_empty());
    }

    #[test]
    fn binary_marker_is_recorded(path in "[a-z]{1,12}\\.bin") {
        let mut diff = String::new();
        diff.push_str(&format!("diff --git a/{} b/{}\n", path, path));
        diff.push_str(&format!("--- a/{}\n", path));
        diff.push_str(&format!("+++ b/{}\n", path));
        diff.push_str("Binary files differ\n");
        let patch = Patch::parse(diff.as_bytes()).unwrap();
        prop_assert_eq!(patch.files.len(), 1);
        prop_assert!(patch.files[0].binary);
    }
}