use proptest::prelude::*;

use backhopper_core::compat::call_sites::{DynamicCall, extract_dynamic_into};

proptest! {
    #[test]
    fn extract_dynamic_does_not_panic_on_any_text(s in ".*") {
        let mut out: Vec<DynamicCall> = Vec::new();
        extract_dynamic_into(&s, &mut out);
    }

    // Literal lowercase:lowercase calls must never be flagged as dynamic.
    #[test]
    fn lowercase_dotted_call_is_never_dynamic(
        module in "[a-z][a-z0-9_]{0,7}",
        function in "[a-z][a-z0-9_]{0,7}"
    ) {
        let line = format!("    {}:{}(Arg1, Arg2)", module, function);
        let mut out: Vec<DynamicCall> = Vec::new();
        extract_dynamic_into(&line, &mut out);
        prop_assert!(
            out.is_empty(),
            "literal call {} flagged: {:?}",
            line,
            out
        );
    }

    // Any uppercase-led module identifier followed by `:fun(` is
    // variable-dispatch, no matter the function name.
    #[test]
    fn uppercase_module_is_always_variable_dispatch(
        module in "[A-Z][a-zA-Z0-9_]{0,7}",
        function in "[a-z][a-z0-9_]{0,7}"
    ) {
        let line = format!("    {}:{}(X)", module, function);
        let mut out: Vec<DynamicCall> = Vec::new();
        extract_dynamic_into(&line, &mut out);
        prop_assert!(
            out.contains(&DynamicCall::VariableDispatch),
            "uppercase module {} did not register as variable-dispatch: {:?}",
            line,
            out
        );
    }
}
