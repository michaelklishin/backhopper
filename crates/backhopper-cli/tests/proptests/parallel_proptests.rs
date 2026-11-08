// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;

use backhopper_cli::commands::parallel::{Jobs, run_in_parallel};

proptest! {
    #[test]
    fn any_worker_count_gives_the_sequential_result(
        items in prop::collection::vec(any::<i32>(), 0..80),
        workers in 1usize..12,
    ) {
        let jobs: Jobs = workers.to_string().parse().unwrap();
        let parallel = run_in_parallel(jobs, &items, || (), |(), n| i64::from(*n) * 3, |_, _| {});
        let sequential: Vec<i64> = items.iter().map(|n| i64::from(*n) * 3).collect();
        prop_assert_eq!(parallel, sequential);
    }

    #[test]
    fn a_worker_count_parses_exactly_when_it_is_at_least_one(n in 0usize..1000) {
        prop_assert_eq!(n.to_string().parse::<Jobs>().is_ok(), n >= 1);
    }
}
