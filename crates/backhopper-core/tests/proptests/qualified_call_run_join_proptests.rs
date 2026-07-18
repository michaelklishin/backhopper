// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The 039 keystone restated for `extract_qualified_calls`: joining
//! file-adjacent runs must change nothing for single-line calls.
//! `a_context_classified_attribute_never_yields_a_call_reference` is
//! the HF-45 property (055): whatever `added_lines_with_context`
//! classifies as a type attribute must never surface as a qualified
//! call, whether its opener sits in `Context` or `Added`.

use proptest::prelude::*;

use backhopper_core::compat::added_lines::added_lines_with_context;
use backhopper_core::compat::call_sites::extract_qualified_calls;
use backhopper_core::compat::call_sites::extract_qualified_calls_with_context;
use backhopper_core::compat::patch::{Hunk, HunkLine};
use backhopper_core::model::symbol::RefContext;

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

    #[test]
    fn a_context_classified_attribute_never_yields_a_call_reference(
        opener_is_context in any::<bool>(),
        module in "[a-z][a-z0-9_]{0,8}",
        ident in "[a-z][a-z0-9_]{0,8}",
    ) {
        let opener_text = "-spec f(X) -> ok when".to_owned();
        let opener = if opener_is_context {
            HunkLine::Context(opener_text)
        } else {
            HunkLine::Added(opener_text)
        };
        let continuation = HunkLine::Added(format!("      X :: {module}:{ident}()."));
        let hunk = Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 2,
            lines: vec![opener, continuation],
        };
        let (added, line_map, ctx) = added_lines_with_context(std::slice::from_ref(&hunk));
        let calls = extract_qualified_calls_with_context(&added, &line_map, &ctx);
        for (i, c) in ctx.iter().enumerate() {
            if *c == RefContext::TypeAttribute {
                let file_line = line_map[i];
                prop_assert!(calls.iter().all(|call| call.line != file_line));
            }
        }
    }
}
