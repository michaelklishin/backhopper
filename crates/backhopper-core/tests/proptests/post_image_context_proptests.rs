// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! With a post-image, an added line classifies as it would in the whole
//! file, however little of the file its hunk carries.

use proptest::prelude::*;

use backhopper_core::compat::added_lines::{LineContextSource, added_lines_with_context};
use backhopper_core::compat::patch::{Hunk, HunkLine};

fn source_line() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "-type config() ::",
        "-spec start(Opts :: map(),",
        "-record(state, {name :: binary(),",
        "    #{timeout => timeout(),",
        "      port => inet:port_number(),",
        "      notify => pid() | none}.",
        "           Pid :: pid()) -> ok.",
        "                count = 0 :: non_neg_integer()}).",
        "start(Opts) ->",
        "    helper(Opts).",
        "%% a comment",
        "",
    ])
}

fn whole_file_hunk(lines: &[&str]) -> Hunk {
    let lines: Vec<HunkLine> = lines.iter().map(|l| HunkLine::Added((*l).into())).collect();
    Hunk {
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: lines.len(),
        lines,
    }
}

fn carved_hunk(lines: &[&str], start: usize, len: usize, context: usize) -> Hunk {
    let lo = start.saturating_sub(context);
    let hi = (start + len + context).min(lines.len());
    let hunk_lines: Vec<HunkLine> = (lo..hi)
        .map(|i| {
            let text = lines[i].to_owned();
            if (start..start + len).contains(&i) {
                HunkLine::Added(text)
            } else {
                HunkLine::Context(text)
            }
        })
        .collect();
    Hunk {
        old_start: lo + 1,
        old_count: 0,
        new_start: lo + 1,
        new_count: hunk_lines.len(),
        lines: hunk_lines,
    }
}

proptest! {
    #[test]
    fn a_carved_hunk_classifies_like_the_whole_file(
        lines in prop::collection::vec(source_line(), 1..30),
        start_seed in any::<prop::sample::Index>(),
        len in 1usize..6,
        context in 0usize..4,
    ) {
        let start = start_seed.index(lines.len());
        let len = len.min(lines.len() - start);
        let text = lines.join("\n") + "\n";
        let (_, _, whole) =
            added_lines_with_context(&[whole_file_hunk(&lines)], LineContextSource::HunksOnly);
        let (_, map, carved) = added_lines_with_context(
            &[carved_hunk(&lines, start, len, context)],
            LineContextSource::PostImage(&text),
        );
        let expected: Vec<_> = map.iter().map(|&n| whole[n as usize - 1].clone()).collect();
        prop_assert_eq!(carved, expected);
    }
}
