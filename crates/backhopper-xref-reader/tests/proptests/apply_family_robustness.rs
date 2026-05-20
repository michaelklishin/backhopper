// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_xref_reader::SourceReader;

fn read_does_not_panic(source: &str) {
    let reader = SourceReader::new();
    let _ = reader.read_one(&PathBuf::from("p.erl"), source);
}

proptest! {
    /// Apply-family resolution must not panic on arbitrary literal-arg
    /// arities; the parser falls back to an unresolved record when the
    /// shape doesn't match.
    #[test]
    fn apply_with_any_arg_count_does_not_panic(
        n_args in 0usize..8,
        module_atom in "[a-z][a-z0-9_]{0,5}",
        function_atom in "[a-z][a-z0-9_]{0,5}",
        list_items in 0usize..8,
    ) {
        let args_list: Vec<String> = (0..list_items).map(|i| format!("item{i}")).collect();
        let args_str: String = (0..n_args).map(|i| match i {
            0 => module_atom.clone(),
            1 => function_atom.clone(),
            2 => format!("[{}]", args_list.join(", ")),
            _ => format!("opt{i}"),
        }).collect::<Vec<_>>().join(", ");
        let source = format!(
            "-module(m).\n\
             -export([go/0]).\n\
             go() -> apply({args_str}).\n"
        );
        read_does_not_panic(&source);
    }

    /// spawn-variants should never panic, regardless of arity.
    #[test]
    fn spawn_variants_do_not_panic(
        family in prop::sample::select(vec!["spawn", "spawn_link", "spawn_monitor", "spawn_opt", "hibernate"]),
        n_args in 0usize..8,
    ) {
        let args = (0..n_args)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "-module(m).\n\
             -export([go/0]).\n\
             go() -> {family}({args}).\n"
        );
        read_does_not_panic(&source);
    }

    /// The exact apply/3 form with all-atom args should always resolve
    /// to a concrete external call.
    #[test]
    fn apply_3_with_literal_atoms_resolves(
        module_atom in "[a-z][a-z0-9_]{0,7}",
        function_atom in "[a-z][a-z0-9_]{0,7}",
        list_arity in 0u8..8,
    ) {
        let list: String = (0..list_arity).map(|i| format!("a{i}")).collect::<Vec<_>>().join(", ");
        let source = format!(
            "-module(m).\n\
             -export([go/0]).\n\
             go() -> apply({module_atom}, {function_atom}, [{list}]).\n"
        );
        let reader = SourceReader::new();
        let (m, _) = reader.read_one(&PathBuf::from("p.erl"), &source).expect("ok");
        let m = m.expect("module");
        let expected = format!("{module_atom}:{function_atom}/{list_arity}");
        let any_match = m.external_calls.iter().any(|c| {
            if let backhopper_xref_graph::CallTarget::External(
                backhopper_xref_graph::FunctionRef::Concrete(mfa),
            ) = &c.callee
            {
                format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity) == expected
            } else {
                false
            }
        });
        prop_assert!(any_match, "expected {} in {:?}", expected, m.external_calls);
    }
}
