// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_driver::ExitClass;
use proptest::prelude::*;

proptest! {
    #[test]
    fn from_code_never_panics(code in proptest::num::i32::ANY) {
        let _ = ExitClass::from_code(code);
    }

    #[test]
    fn unknown_carries_the_original_code(code in proptest::num::i32::ANY) {
        let class = ExitClass::from_code(code);
        if matches!(class, ExitClass::Unknown(c) if c == code) {
            // Property holds: when not a documented code, Unknown wraps it
        } else {
            prop_assert!(matches!(
                code,
                0..=4 | 64 | 65 | 66 | 74
            ));
        }
    }

    #[test]
    fn success_shaped_implies_zero_one_two_three_or_four(code in proptest::num::i32::ANY) {
        let class = ExitClass::from_code(code);
        if class.is_success_shaped() {
            prop_assert!(matches!(code, 0..=4));
        }
    }
}
