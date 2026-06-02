// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_driver::ExitClass;

#[test]
fn verdict_codes_map_to_verdict_outcome() {
    for code in [0, 2, 3, 4] {
        assert_eq!(ExitClass::from_code(code), ExitClass::VerdictOutcome);
    }
}

#[test]
fn code_one_maps_to_success_shaped_query() {
    assert_eq!(ExitClass::from_code(1), ExitClass::SuccessShapedQuery);
}

#[test]
fn sysexits_codes_map_to_named_variants() {
    assert_eq!(ExitClass::from_code(64), ExitClass::UsageError);
    assert_eq!(ExitClass::from_code(65), ExitClass::DataError);
    assert_eq!(ExitClass::from_code(66), ExitClass::NoInput);
    assert_eq!(ExitClass::from_code(74), ExitClass::IoError);
}

#[test]
fn unknown_codes_preserve_the_integer() {
    assert_eq!(ExitClass::from_code(137), ExitClass::Unknown(137));
}

#[test]
fn is_success_shaped_covers_verdict_and_grep() {
    assert!(ExitClass::VerdictOutcome.is_success_shaped());
    assert!(ExitClass::SuccessShapedQuery.is_success_shaped());
    assert!(!ExitClass::UsageError.is_success_shaped());
    assert!(!ExitClass::DataError.is_success_shaped());
    assert!(!ExitClass::Unknown(99).is_success_shaped());
}

#[test]
fn is_verdict_is_strictly_the_zero_two_three_four_set() {
    assert!(ExitClass::VerdictOutcome.is_verdict());
    assert!(!ExitClass::SuccessShapedQuery.is_verdict());
}
