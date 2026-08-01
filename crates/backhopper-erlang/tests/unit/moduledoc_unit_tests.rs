// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! OTP 27 rewrote module documentation into `-moduledoc` and `-doc`
//! attributes carrying triple-quoted strings. Their prose must stay
//! opaque: an attribute region that runs past its closing quotes
//! swallows the exports that follow.

use backhopper_erlang::ErlangExtractor;
use backhopper_erlang::tokenizer::iterate_attributes;

fn names(src: &str) -> Vec<String> {
    iterate_attributes(src)
        .into_iter()
        .map(|b| b.name)
        .collect()
}

#[test]
fn a_moduledoc_block_does_not_swallow_the_exports_after_it() {
    let src = "-module(crypto).\n\
               -moduledoc \"\"\"\n\
               Functions for cryptography.\n\
               \"\"\".\n\
               -export([strong_rand_bytes/1, mac/4]).\n";
    assert_eq!(names(src), vec!["module", "moduledoc", "export"]);
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.exports.len(), 2);
}

#[test]
fn an_export_line_inside_documentation_prose_is_not_an_attribute() {
    let src = "-module(crypto).\n\
               -moduledoc \"\"\"\n\
               -export([phantom/0]).\n\
               \"\"\".\n\
               -export([mac/4]).\n";
    assert_eq!(names(src), vec!["module", "moduledoc", "export"]);
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.exports.len(), 1);
    assert_eq!(m.exports[0].name.as_str(), "mac");
}

// what killed edlin: markdown bullets and inline code leave an odd
// number of quotes behind on a line
#[test]
fn prose_with_unbalanced_quotes_does_not_leak_string_state_past_the_closer() {
    let src = "-module(edlin).\n\
               -moduledoc \"\"\"\n\
               - **`transpose_char`** - swap the \"character behind the cursor.\n\
               \"\"\".\n\
               -export([start/1]).\n";
    assert_eq!(names(src), vec!["module", "moduledoc", "export"]);
}

#[test]
fn a_doc_block_between_two_specs_leaves_both_specs_visible() {
    let src = "-module(crypto).\n\
               -spec mac(atom(), binary(), binary(), binary()) -> binary().\n\
               -doc \"\"\"\n\
               Computes a MAC. See the example below.\n\
               \"\"\".\n\
               -spec strong_rand_bytes(non_neg_integer()) -> binary().\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.specs.len(), 2);
}

// findings render `file:line`, so the count of lines the doc body spans
// has to reach the attributes after it
#[test]
fn attribute_line_numbers_after_a_doc_block_match_the_source() {
    let src = "-module(crypto).\n\
               -moduledoc \"\"\"\n\
               one\n\
               two\n\
               three\n\
               \"\"\".\n\
               -export([mac/4]).\n";
    let blocks = iterate_attributes(src);
    assert_eq!(blocks[1].line, 2);
    assert_eq!(blocks[2].line, 7);
}

#[test]
fn a_fenced_erlang_example_inside_a_doc_block_stays_prose() {
    let src = "-module(crypto).\n\
               -moduledoc \"\"\"\n\
               ```erlang\n\
               1> crypto:strong_rand_bytes(16).\n\
               ```\n\
               \"\"\".\n\
               -export([strong_rand_bytes/1]).\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.exports.len(), 1);
    assert_eq!(m.name.as_str(), "crypto");
}

#[test]
fn an_ordinary_single_quoted_doc_attribute_still_closes_on_its_own_line() {
    let src = "-module(ra_log).\n\
               -doc \"Appends an entry.\".\n\
               -export([append/2]).\n";
    assert_eq!(names(src), vec!["module", "doc", "export"]);
}

// EEP-59 uses `-moduledoc false.` for "not documented", which is not
// the same as hidden: the module keeps its public surface
#[test]
fn moduledoc_false_does_not_hide_the_module() {
    let src = "-module(ra_lib).\n\
               -moduledoc false.\n\
               -export([iter_maybe/2]).\n";
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert_eq!(m.exports.len(), 1);
}

// the compiler rejects an unterminated opener, so consuming the rest of
// the file is what a real parse concludes; nothing after it may be read
// as an attribute
#[test]
fn an_unterminated_doc_block_swallows_the_rest_and_invents_nothing() {
    let src = "-module(crypto).\n\
               -moduledoc \"\"\"\n\
               Never closed.\n\
               -export([phantom/0]).\n";
    assert_eq!(names(src), vec!["module", "moduledoc"]);
    let m = ErlangExtractor::default().extract_module(src).unwrap();
    assert!(m.exports.is_empty());
}
