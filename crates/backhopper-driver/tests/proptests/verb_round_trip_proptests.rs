// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_driver::Verb;
use proptest::prelude::*;

fn verb_strategy() -> impl Strategy<Value = Verb> {
    let all: Vec<Verb> = Verb::iter().collect();
    proptest::sample::select(all)
}

proptest! {
    #[test]
    fn parse_round_trip_for_every_verb(verb in verb_strategy()) {
        let display = verb.display_name();
        prop_assert_eq!(Verb::parse(&display).unwrap(), verb);
        let serialised = serde_json::to_string(&verb).unwrap();
        let back: Verb = serde_json::from_str(&serialised).unwrap();
        prop_assert_eq!(back, verb);
    }

    #[test]
    fn arbitrary_strings_never_panic(s in ".*") {
        let _ = Verb::parse(&s);
    }
}
