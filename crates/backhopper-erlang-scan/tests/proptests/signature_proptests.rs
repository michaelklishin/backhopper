// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_erlang_scan::parse_callable_signature;

proptest! {
    #[test]
    fn arity_matches_plain_argument_count(n in 0usize..8) {
        let args: Vec<String> = (0..n).map(|i| format!("A{i} :: term()")).collect();
        let body = format!("handle({}) -> ok", args.join(", "));
        let sig = parse_callable_signature(&body).unwrap();
        prop_assert_eq!(sig.arity as usize, n);
    }

    #[test]
    fn bitstring_arguments_do_not_inflate_arity(n in 1usize..6) {
        let args: Vec<String> = (0..n).map(|_| "<<_:8, _:_*8>>".to_string()).collect();
        let body = format!("encode({}) -> binary()", args.join(", "));
        let sig = parse_callable_signature(&body).unwrap();
        prop_assert_eq!(sig.arity as usize, n);
    }

    #[test]
    fn never_panics_on_arbitrary_input(body in ".{0,200}") {
        let _ = parse_callable_signature(&body);
    }
}
