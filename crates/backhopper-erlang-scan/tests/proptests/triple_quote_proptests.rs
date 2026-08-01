// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_erlang_scan::{ScanArity, scan_arity, triple_quoted_span};

// documentation prose is where the shapes that desync a one-quote-at-a-time
// scanner live: unbalanced quotes, sentence-ending dots, markdown bullets at
// column zero, and attribute-shaped lines inside fenced examples
fn prose_line() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "Swap the character behind the cursor.".to_string(),
        "- **`transpose_char`** - swap two characters".to_string(),
        "he said \"hello and never stopped".to_string(),
        "-export([phantom/0]).".to_string(),
        "1> crypto:strong_rand_bytes(16).".to_string(),
        "a, b, c".to_string(),
        "end end end".to_string(),
        "```erlang".to_string(),
        String::new(),
    ])
}

proptest! {
    // callers advance the cursor by the span, so one past the end would
    // put every scanner that consults it out of bounds
    #[test]
    fn a_span_never_reaches_past_the_end_of_the_input(bytes: Vec<u8>, at in 0usize..64) {
        if let Some(span) = triple_quoted_span(&bytes, at) {
            prop_assert!(at + span <= bytes.len());
        }
    }

    // the span is delimited by quote runs at both ends whenever the input
    // holds a closer at all
    #[test]
    fn a_returned_span_starts_and_ends_on_quote_runs(lines in prop::collection::vec(prose_line(), 0..8)) {
        let src = format!("\"\"\"\n{}\n\"\"\"", lines.join("\n"));
        let span = triple_quoted_span(src.as_bytes(), 0).unwrap();
        prop_assert!(src[..span].starts_with("\"\"\""));
        prop_assert!(src[..span].ends_with("\"\"\""));
    }

    // a triple-quoted argument counts as exactly one argument no matter
    // what its content looks like
    #[test]
    fn a_triple_quoted_argument_counts_as_one(
        lines in prop::collection::vec(prose_line(), 0..8),
        before in 0usize..3,
        after in 0usize..3,
    ) {
        let lead: String = "ok, ".repeat(before);
        let trail: String = ", ok".repeat(after);
        let doc = format!("\"\"\"\n{}\n\"\"\"", lines.join("\n"));
        let with_doc = format!("{lead}{doc}{trail})");
        let with_atom = format!("{lead}doc{trail})");
        prop_assert_eq!(scan_arity(&with_doc), scan_arity(&with_atom));
        prop_assert_eq!(scan_arity(&with_doc), ScanArity::Exact((before + after + 1) as u8));
    }
}
