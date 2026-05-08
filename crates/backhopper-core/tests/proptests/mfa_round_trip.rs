use proptest::prelude::*;

use backhopper_core::model::names::Mfa;

fn arb_lower_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}".prop_map(|s| s)
}

fn arb_arity() -> impl Strategy<Value = u8> {
    0u8..=20
}

proptest! {
    #[test]
    fn mfa_from_str_round_trip(
        m in arb_lower_atom(),
        f in arb_lower_atom(),
        a in arb_arity()
    ) {
        let s = format!("{}:{}/{}", m, f, a);
        let parsed: Mfa = s.parse().unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }
}
