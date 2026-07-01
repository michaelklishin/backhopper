// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_specs` scans arbitrary bytes without panicking, and every
//! signature it stores keeps the `->` half, so the return-shape
//! comparator can always find the arrow (a table of arrow-less
//! signatures would silently withhold every comparison).

use proptest::prelude::*;

use backhopper_core::compat::source_attributes::extract_specs;
use backhopper_core::model::spec_ast::SpecType;
use backhopper_core::model::spec_parser::parse_signature_return;

proptest! {
    #[test]
    fn never_panics_on_arbitrary_input(src in ".{0,400}") {
        let _ = extract_specs(&src);
    }

    #[test]
    fn every_stored_signature_contains_an_arrow(
        name in "[a-z][a-z_]{0,12}",
        ret in "(ok|binary\\(\\)|list\\(\\)|\\{ok, term\\(\\)\\})",
    ) {
        let src = format!("-spec {name}(term()) -> {ret}.\n");
        let specs = extract_specs(&src);
        prop_assert_eq!(specs.len(), 1);
        for signature in specs.values() {
            prop_assert!(signature.contains("->"));
            prop_assert!(!matches!(
                parse_signature_return(signature),
                SpecType::Unknown
            ));
        }
    }

    // A spec table keyed by the shared parser agrees with the snapshot
    // grammar on bitstring arity: n bitstring arguments key as arity n.
    #[test]
    fn bitstring_arguments_key_at_the_snapshot_arity(n in 1u8..5) {
        let args: Vec<String> = (0..n).map(|_| "<<_:8, _:_*8>>".to_string()).collect();
        let src = format!("-spec encode({}) -> binary().\n", args.join(", "));
        let specs = extract_specs(&src);
        prop_assert_eq!(specs.len(), 1);
        let (_, arity) = specs.keys().next().unwrap();
        prop_assert_eq!(arity.get(), n);
    }
}
