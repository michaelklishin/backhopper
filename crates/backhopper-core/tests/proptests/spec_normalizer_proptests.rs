use proptest::prelude::*;

use backhopper_core::snapshot::spec_normalize::normalize_signature;

proptest! {
    #[test]
    fn normalizer_is_idempotent(s in "[a-zA-Z0-9_(){}\\[\\]:|, \\->]{0,80}") {
        let once = normalize_signature(&s);
        let twice = normalize_signature(&once);
        prop_assert_eq!(once, twice);
    }
}
