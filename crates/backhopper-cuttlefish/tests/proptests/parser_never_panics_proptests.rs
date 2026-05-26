// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;
use std::str;

use proptest::prelude::*;

use backhopper_cuttlefish::parse_schema;

proptest! {
    #[test]
    fn parser_never_panics_on_arbitrary_ascii(s in "[\\x20-\\x7e\\n%]{0,400}") {
        let _ = parse_schema(&s, Path::new("fuzz.schema"));
    }

    #[test]
    fn parser_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..500)) {
        if let Ok(s) = str::from_utf8(&bytes) {
            let _ = parse_schema(s, Path::new("fuzz.schema"));
        }
    }

    #[test]
    fn fragments_count_at_least_zero(
        runs in 1usize..10,
        body in "[a-z_]{3,10}"
    ) {
        // Build a schema body with `runs` mapping tuples. Parser must
        // return exactly `runs` fragments.
        let mut s = String::new();
        for i in 0..runs {
            s.push_str(&format!("{{mapping, \"k{i}\", \"v{i}\", [{body}]}}.\n"));
        }
        let frags = parse_schema(&s, Path::new("loop.schema")).unwrap();
        prop_assert_eq!(frags.len(), runs);
    }
}
