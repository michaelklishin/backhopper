// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The reader runs over every file of every tag of every project, so
//! arbitrary text — including multibyte UTF-8 in comments, strings,
//! and `$c` char literals — must never panic the scanner.

use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_xref_reader::SourceReader;

proptest! {
    #[test]
    fn read_one_does_not_panic_on_unicode_garbage(s in "[\\PC]{0,2048}") {
        let reader = SourceReader::new();
        let _ = reader.read_one(&PathBuf::from("p.erl"), &s);
    }

    /// Multibyte text in the places real OTP sources carry it: a
    /// comment, a string, a quoted atom, and a `$c` char literal.
    #[test]
    fn read_one_survives_multibyte_in_erlang_positions(word in "[€йä漢]{1,8}") {
        let source = format!(
            "%% автор: {word}\n\
             -module(m).\n\
             -export([f/0]).\n\
             f() -> {{\"{word}\", '{word}', $€, other:call(1)}}.\n"
        );
        let reader = SourceReader::new();
        let parsed = reader.read_one(&PathBuf::from("p.erl"), &source);
        prop_assert!(parsed.is_ok());
    }
}
