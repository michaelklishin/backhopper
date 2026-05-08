use proptest::prelude::*;

use backhopper_erlang::ErlangExtractor;

proptest! {
    #[test]
    fn extract_module_does_not_panic_on_garbage(s in "[\\PC]{0,2048}") {
        let ex = ErlangExtractor::default();
        let _ = ex.extract_module(&s);
    }

    #[test]
    fn extract_header_does_not_panic_on_garbage(s in "[\\PC]{0,2048}") {
        let ex = ErlangExtractor::default();
        let _ = ex.extract_header_file("/tmp/garbage.hrl", &s);
    }
}
