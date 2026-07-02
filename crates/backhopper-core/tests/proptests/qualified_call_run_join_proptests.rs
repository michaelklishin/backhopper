// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The 039 keystone restated for `extract_qualified_calls`: joining
//! file-adjacent runs must change nothing for single-line calls.

use proptest::prelude::*;

use backhopper_core::compat::call_sites::extract_qualified_calls;

fn seq_map(src: &str) -> Vec<u32> {
    (1..=src.lines().count() as u32).collect()
}

proptest! {
    #[test]
    fn joined_runs_equal_isolated_lines_for_single_line_calls(
        n in 1usize..6,
        arity in 0usize..4,
    ) {
        let args: Vec<String> = (0..arity).map(|i| format!("A{i}")).collect();
        let lines: Vec<String> = (0..n)
            .map(|i| format!("f{i}(X) -> rabbit_misc:format_{i}({}).", args.join(", ")))
            .collect();
        let src = lines.join("\n") + "\n";
        // Adjacent map: one joined run.
        let joined = extract_qualified_calls(&src, &seq_map(&src));
        // Every line 10 apart: n single-line runs.
        let apart: Vec<u32> = (0..n as u32).map(|i| 1 + i * 10).collect();
        let isolated = extract_qualified_calls(&src, &apart);
        prop_assert_eq!(joined, isolated);
    }

    #[test]
    fn never_panics_on_arbitrary_text(src in "[ -~\n]{0,200}") {
        let map = seq_map(&src);
        let _ = extract_qualified_calls(&src, &map);
    }
}
