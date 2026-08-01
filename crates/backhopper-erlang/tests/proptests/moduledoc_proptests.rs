// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_erlang::tokenizer::iterate_attributes;

// the shapes that decide whether a documented module survives: whether
// stray quotes, sentence dots, column-zero dashes, and attribute-looking
// lines fall inside the prose
fn prose_line() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "Functions for public-key infrastructure.".to_string(),
        "- **`transpose_char`** - Swap the character behind the cursor.".to_string(),
        "he said \"hello and never closed the quote".to_string(),
        "-export([phantom/0]).".to_string(),
        "-spec phantom() -> ok.".to_string(),
        "1> crypto:strong_rand_bytes(16).".to_string(),
        "```erlang".to_string(),
        "%% not a comment in here".to_string(),
        "{[(unbalanced".to_string(),
        String::new(),
    ])
}

proptest! {
    // HF-52's shape directly: the attributes after a documentation block
    // are found exactly as they are when the block is absent
    #[test]
    fn a_documentation_block_is_transparent_to_the_attributes_after_it(
        lines in prop::collection::vec(prose_line(), 0..10),
    ) {
        let tail = "-export([strong_rand_bytes/1, mac/4]).\n-spec mac(atom()) -> binary().\n";
        let with_doc = format!(
            "-module(crypto).\n-moduledoc \"\"\"\n{}\n\"\"\".\n{tail}",
            lines.join("\n")
        );
        let without_doc = format!("-module(crypto).\n{tail}");

        let after_doc: Vec<_> = iterate_attributes(&with_doc)
            .into_iter()
            .filter(|b| b.name != "moduledoc")
            .map(|b| (b.name, b.body))
            .collect();
        let baseline: Vec<_> = iterate_attributes(&without_doc)
            .into_iter()
            .map(|b| (b.name, b.body))
            .collect();
        prop_assert_eq!(after_doc, baseline);
    }
}
