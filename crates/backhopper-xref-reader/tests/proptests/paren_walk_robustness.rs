// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_xref_reader::scanner::{Scanner, arity_from_commas, walk_paren_group};
use proptest::prelude::*;

proptest! {
    #[test]
    fn arity_equals_top_level_argument_count(
        args in prop::collection::vec("[a-z][a-z0-9_]{0,5}", 0..8),
    ) {
        let src = format!("({})", args.join(", "));
        let mut sc = Scanner::new(&src);
        let walk = walk_paren_group(&mut sc, |_| false);
        prop_assert!(walk.closed);
        let arity = arity_from_commas(walk.commas, walk.had_content);
        prop_assert_eq!(arity, args.len() as u32);
    }

    #[test]
    fn interior_commas_inside_brackets_are_not_counted(
        groups in prop::collection::vec(
            (
                prop::sample::select(vec!['[', '{']),
                prop::collection::vec("[a-z]{1,4}", 1..4),
            ),
            1..5,
        ),
    ) {
        let rendered: Vec<String> = groups
            .iter()
            .map(|(open, items)| {
                let close = if *open == '[' { ']' } else { '}' };
                format!("{open}{}{close}", items.join(", "))
            })
            .collect();
        let src = format!("({})", rendered.join(", "));
        let mut sc = Scanner::new(&src);
        let walk = walk_paren_group(&mut sc, |_| false);
        prop_assert!(walk.closed);
        let arity = arity_from_commas(walk.commas, walk.had_content);
        prop_assert_eq!(arity, groups.len() as u32);
    }
}
