// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Generated argument lists mixing block expressions with ordinary
//! terms: the arity read must always equal the number of arguments
//! built, however many commas the block bodies carry.

use proptest::prelude::*;

use backhopper_erlang_scan::{
    ScanArity, arity_of_args, parse_callable_signature, scan_arity, split_top_level_args,
};

fn arb_fun_expr() -> impl Strategy<Value = String> {
    (1usize..4, prop::bool::ANY, prop::bool::ANY).prop_map(|(stmts, guarded, named)| {
        let body = (0..stmts)
            .map(|i| format!("step_{i}(X)"))
            .collect::<Vec<_>>()
            .join(",\n        ");
        let head = match (named, guarded) {
            (true, _) => "fun Loop(X) ->",
            (false, true) => "fun(X) when X > 0 ->",
            (false, false) => "fun(X) ->",
        };
        format!("{head}\n        {body}\n    end")
    })
}

fn arb_plain_term() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "Config",
        "[a, b]",
        "#{node => Node}",
        "rabbit_queue",
        "{ok, 1}",
        "fun rabbit_misc:pget/2",
        "case X of a -> 1, 2; b -> 3 end",
        "maybe V ?= f(X), V else _ -> err end",
        "maybe A = g(X), h(A), A end",
        "#end{a = 1, b = 2}",
        "#'try'{x = 1}",
        "State#state.field",
    ])
    .prop_map(str::to_owned)
}

fn arb_arg() -> impl Strategy<Value = String> {
    prop_oneof![arb_plain_term(), arb_fun_expr()]
}

fn arb_spec_type() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "map()",
        "[binary()]",
        "fun()",
        "fun((rabbit_types:amqqueue()) -> ok)",
        "fun((a, b) -> {ok, c})",
        "pos_integer()",
    ])
    .prop_map(str::to_owned)
}

proptest! {
    #[test]
    fn arity_matches_the_generated_argument_count(args in prop::collection::vec(arb_arg(), 1..6)) {
        let inner = args.join(", ");
        prop_assert_eq!(
            scan_arity(&format!("{inner})")),
            ScanArity::Exact(args.len() as u8)
        );
        prop_assert_eq!(arity_of_args(&inner), args.len());
    }

    #[test]
    fn splitting_and_counting_agree(args in prop::collection::vec(arb_arg(), 1..6)) {
        let inner = args.join(", ");
        prop_assert_eq!(split_top_level_args(&inner).len(), arity_of_args(&inner));
    }

    // fun types close with no `end`: spec arities must not shift
    #[test]
    fn spec_signatures_are_unchanged_by_the_block_rule(
        types in prop::collection::vec(arb_spec_type(), 0..5),
    ) {
        let body = format!("declare({}) -> ok", types.join(", "));
        let parsed = parse_callable_signature(&body).expect("parses");
        prop_assert_eq!(usize::from(parsed.arity), types.len());
    }
}
