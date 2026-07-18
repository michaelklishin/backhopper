// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The symbol scanners run on arbitrary malformed source: arbitrary bytes,
//! truncated directives, nested sigils. They must never panic.

use proptest::prelude::*;

use backhopper_core::compat::source_attributes::{
    declares_parse_transform, extract_defined_macros, extract_defined_records,
    extract_function_signatures, extract_imports, extract_macro_uses, extract_record_uses,
};

proptest! {
    #[test]
    fn scanners_never_panic_on_arbitrary_text(s in ".{0,400}") {
        let _ = extract_macro_uses(&s);
        let _ = extract_defined_macros(&s);
        let _ = extract_record_uses(&s);
        let _ = extract_defined_records(&s);
        let _ = extract_function_signatures(&s);
        let _ = extract_imports(&s);
        let _ = declares_parse_transform(&s);
    }

    // A bare ?NAME before a non-name byte is always found, with the exact identifier as its name.
    #[test]
    fn a_macro_use_is_recovered(name in "[A-Z][A-Z0-9_]{0,20}") {
        let src = format!("f() -> ?{name}.\n");
        let uses = extract_macro_uses(&src);
        prop_assert!(uses.iter().any(|u| u.name == name));
    }
}
