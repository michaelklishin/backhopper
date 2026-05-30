// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::spec_parser::{parse, parse_signature_return};
use proptest::prelude::*;

fn arb_spec_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_(){}\\[\\] ,:|@<>='#?\\-/.\\\\\"]{0,160}".prop_map(|s| s)
}

proptest! {
    #[test]
    fn parse_never_panics(text in arb_spec_text()) {
        let _ = parse(&text);
    }

    #[test]
    fn parse_signature_return_never_panics(text in arb_spec_text()) {
        let _ = parse_signature_return(&text);
    }

    #[test]
    fn canonicalise_is_idempotent(text in arb_spec_text()) {
        let a = parse(&text);
        let b = a.clone().canonicalise();
        prop_assert_eq!(a, b);
    }
}
