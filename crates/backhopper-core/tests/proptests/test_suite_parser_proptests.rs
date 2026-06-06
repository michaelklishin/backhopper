// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `TestSuiteFile<Raw>::parse` must not panic on arbitrary input. The
//! resolver is downstream; if the parser blows up, the entire
//! diagnostic and verdict pipeline goes with it. Bounded input string
//! length keeps the proptest fast.

use backhopper_core::compat::test_suite::TestSuiteFile;
use backhopper_core::model::names::RelativePath;
use proptest::prelude::*;

fn arbitrary_suite_source() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..2048)
        .prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        ..ProptestConfig::default()
    })]

    #[test]
    fn parse_does_not_panic_on_arbitrary_bytes(src in arbitrary_suite_source()) {
        let suite = RelativePath::new("a/x_SUITE.erl").unwrap();
        let _ = TestSuiteFile::new(suite, src).parse();
    }

    #[test]
    fn parse_does_not_panic_on_lots_of_quotes(
        n in 0usize..200,
        ch in prop_oneof![Just('"'), Just('\''), Just('%'), Just('('), Just(')'), Just(':')],
    ) {
        let src: String = std::iter::repeat_n(ch, n).collect();
        let suite = RelativePath::new("a/x_SUITE.erl").unwrap();
        let _ = TestSuiteFile::new(suite, src).parse();
    }

    #[test]
    fn parse_then_referenced_modules_returns_subset_of_calls(
        src in arbitrary_suite_source()
    ) {
        let suite = RelativePath::new("a/x_SUITE.erl").unwrap();
        if let Ok(parsed) = TestSuiteFile::new(suite, src).parse() {
            let mods = parsed.referenced_modules();
            // Every referenced module appears at least once in the call list.
            for m in &mods {
                prop_assert!(parsed.calls().iter().any(|c| c.module == *m));
            }
            // Referenced-modules list is deduplicated.
            for i in 0..mods.len() {
                for j in (i + 1)..mods.len() {
                    prop_assert_ne!(&mods[i], &mods[j]);
                }
            }
        }
    }
}
