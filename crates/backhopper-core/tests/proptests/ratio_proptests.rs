// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::eval::Ratio;
use proptest::prelude::*;

proptest! {
    #[test]
    fn hits_never_exceed_total_after_any_sequence_of_hit_and_miss(calls in prop::collection::vec(any::<bool>(), 0..64)) {
        let mut r = Ratio::zero();
        for is_hit in calls {
            if is_hit {
                r.hit();
            } else {
                r.miss();
            }
        }
        prop_assert!(r.hits() <= r.total());
        match r.rate() {
            None => prop_assert_eq!(r.total(), 0),
            Some(rate) => {
                prop_assert!(r.total() > 0);
                prop_assert!((0.0..=1.0).contains(&rate));
            }
        }
    }
}

#[test]
fn rate_is_none_exactly_at_zero_total() {
    assert_eq!(Ratio::zero().rate(), None);
    let mut r = Ratio::zero();
    r.miss();
    assert_eq!(r.rate(), Some(0.0));
    r.hit();
    assert_eq!(r.rate(), Some(0.5));
}
