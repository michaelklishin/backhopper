// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-MIT and LICENSE-APACHE for details.

use proptest::prelude::*;

use backhopper_erlang_scan::{ScanArity, scan_arity, sigil_span};

const DELIMS: [(char, char); 10] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('/', '/'),
    ('|', '|'),
    ('#', '#'),
    ('`', '`'),
    ('\'', '\''),
    ('"', '"'),
];

fn prefix() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["", "s", "S", "b", "B", "r", "MY@sigil"]).prop_map(str::to_string)
}

// content that legally desyncs naive scanners: quotes, commas,
// parens, block keywords
fn content() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "",
        "a \" b",
        "x , y",
        "( [ {",
        "end case fun",
        "a.b.",
        "%not a comment",
    ])
    .prop_map(str::to_string)
}

proptest! {
    #[test]
    fn a_span_never_reaches_past_the_end_of_the_input(bytes: Vec<u8>, at in 0usize..64) {
        if bytes.get(at) == Some(&b'~')
            && let Some(span) = sigil_span(&bytes, at) {
                prop_assert!(at + span <= bytes.len());
                prop_assert!(span >= 1);
            }
    }

    #[test]
    fn a_sigil_argument_counts_like_an_atom(
        pre in prefix(),
        body in content(),
        di in 0usize..DELIMS.len(),
    ) {
        let (open, close) = DELIMS[di];
        // skip bodies that contain the closer or escape it
        prop_assume!(!body.contains(close) && !body.contains('\\'));
        let with_sigil = format!("~{pre}{open}{body}{close}, second)");
        prop_assert_eq!(scan_arity(&with_sigil), ScanArity::Exact(2));
        prop_assert_eq!(scan_arity("stub, second)"), ScanArity::Exact(2));
    }
}
