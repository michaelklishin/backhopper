use std::path::PathBuf;

use proptest::prelude::*;

use backhopper_xref_graph::{CallTarget, FunctionRef};
use backhopper_xref_reader::SourceReader;

fn external_set(m: &backhopper_xref_reader::ModuleData) -> std::collections::BTreeSet<String> {
    m.external_calls
        .iter()
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                Some(format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity))
            }
            _ => None,
        })
        .collect()
}

proptest! {
    /// Two-level nesting must surface both the outer and inner remote
    /// calls. The shape is `outer:f(inner:g())`.
    #[test]
    fn two_level_chain_records_both(
        outer_m in "[a-z][a-z0-9_]{0,4}",
        outer_f in "[a-z][a-z0-9_]{0,4}",
        inner_m in "[a-z][a-z0-9_]{0,4}",
        inner_f in "[a-z][a-z0-9_]{0,4}",
    ) {
        let source = format!(
            "-module(m).\n\
             -export([go/0]).\n\
             go() -> {outer_m}:{outer_f}({inner_m}:{inner_f}()).\n"
        );
        let reader = SourceReader::new();
        let (m, _) = reader.read_one(&PathBuf::from("p.erl"), &source).expect("ok");
        let m = m.expect("module");
        let calls = external_set(&m);
        let outer = format!("{outer_m}:{outer_f}/1");
        let inner = format!("{inner_m}:{inner_f}/0");
        prop_assert!(calls.contains(&outer));
        prop_assert!(calls.contains(&inner));
    }

    /// Deeply nested remote calls all surface as concrete external
    /// calls under the same outer caller.
    #[test]
    fn three_level_chain_records_all(
        a in "[a-z][a-z0-9_]{0,3}",
        b in "[a-z][a-z0-9_]{0,3}",
        c in "[a-z][a-z0-9_]{0,3}",
    ) {
        let source = format!(
            "-module(m).\n\
             -export([go/0]).\n\
             go() -> {a}:f({b}:g({c}:h())).\n"
        );
        let reader = SourceReader::new();
        let (m, _) = reader.read_one(&PathBuf::from("p.erl"), &source).expect("ok");
        let m = m.expect("module");
        let calls = external_set(&m);
        let want_a = format!("{a}:f/1");
        let want_b = format!("{b}:g/1");
        let want_c = format!("{c}:h/0");
        prop_assert!(calls.contains(&want_a));
        prop_assert!(calls.contains(&want_b));
        prop_assert!(calls.contains(&want_c));
    }
}
