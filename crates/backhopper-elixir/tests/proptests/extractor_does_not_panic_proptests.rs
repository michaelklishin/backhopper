use proptest::prelude::*;

use backhopper_elixir::ElixirExtractor;

proptest! {
    #[test]
    fn extract_modules_does_not_panic_on_garbage(s in "[\\PC]{0,2048}") {
        let ex = ElixirExtractor::default();
        let _ = ex.extract_modules(&s);
    }

    #[test]
    fn well_formed_module_yields_at_least_one(name in "[A-Z][A-Za-z0-9]{0,8}") {
        let src = format!("defmodule {} do\nend\n", name);
        let ex = ElixirExtractor::default();
        let modules = ex.extract_modules(&src);
        prop_assert!(!modules.is_empty());
        prop_assert_eq!(modules[0].name.as_str(), name.as_str());
    }
}
