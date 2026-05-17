use proptest::prelude::*;

use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::extract_into;

fn mfas(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    extract_into(source, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

proptest! {
    /// apply/3 with literal atoms and a literal list of `n` items
    /// must resolve to `module:function/n`.
    #[test]
    fn apply_3_with_atom_literals_always_resolves(
        module in "[a-z][a-z0-9_]{0,7}",
        function in "[a-z][a-z0-9_]{0,7}",
        list_arity in 0u8..8,
    ) {
        let list = (0..list_arity)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("apply({module}, {function}, [{list}])");
        let calls = mfas(&source);
        let expected = format!("{module}:{function}/{list_arity}");
        prop_assert!(calls.contains(&expected), "want {expected} in {calls:?}");
    }

    /// All spawn-family variants with literal arity-3 form should
    /// resolve into `M:F/length(A)`.
    #[test]
    fn spawn_family_with_atom_literals_resolves(
        family in prop::sample::select(vec![
            "spawn",
            "spawn_link",
            "spawn_monitor",
            "hibernate",
        ]),
        module in "[a-z][a-z0-9_]{0,5}",
        function in "[a-z][a-z0-9_]{0,5}",
        list_arity in 0u8..6,
    ) {
        let list = (0..list_arity)
            .map(|i| format!("a{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("{family}({module}, {function}, [{list}])");
        let calls = mfas(&source);
        let expected = format!("{module}:{function}/{list_arity}");
        prop_assert!(calls.contains(&expected), "want {expected} in {calls:?}");
    }

    /// extract_into must never panic on arbitrary printable input.
    #[test]
    fn extract_into_is_panic_free(text in "[\\x20-\\x7e]{0,256}") {
        let _ = mfas(&text);
    }
}
