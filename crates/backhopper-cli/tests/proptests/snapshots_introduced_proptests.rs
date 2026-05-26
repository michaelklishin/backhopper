// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants of `compute_introduced_rows`:
//!  * `first_tag` is always the earliest tag whose presence bit is set; `last_tag` the latest
//!  * `tags_present` equals the count of `true` presence bits
//!  * `--timeline` always has length equal to the walk length and matches presence bit-for-bit
//!  * adding a never-present MFA never disturbs another MFA's row

use std::iter;

use proptest::prelude::*;

use backhopper_cli::commands::snapshots::{TagSnapshot, compute_introduced_rows};
use backhopper_core::model::names::{CommitSha, Mfa, TagName};

fn sha(byte: u8) -> CommitSha {
    let s: String = iter::repeat_n(format!("{byte:02x}"), 20).collect();
    CommitSha::new(s).unwrap()
}

fn tag_for(i: usize) -> TagName {
    format!("v{i}.0.0").parse().unwrap()
}

fn build_walk(presence_per_tag: &[bool]) -> Vec<TagSnapshot> {
    presence_per_tag
        .iter()
        .enumerate()
        .map(|(i, &p)| TagSnapshot {
            tag: tag_for(i),
            commit: sha((i % 250) as u8),
            presence: vec![p],
        })
        .collect()
}

proptest! {
    #[test]
    fn count_matches_set_bits_in_presence_vector(
        bits in prop::collection::vec(any::<bool>(), 0..32)
    ) {
        let walk = build_walk(&bits);
        let mfa: Mfa = "m:f/0".parse().unwrap();
        let rows = compute_introduced_rows(&walk, &[mfa], false);
        let expected = bits.iter().filter(|b| **b).count();
        prop_assert_eq!(rows[0].tags_present, expected);
    }

    #[test]
    fn first_tag_is_the_lowest_index_with_a_true_bit(
        bits in prop::collection::vec(any::<bool>(), 1..32)
    ) {
        let walk = build_walk(&bits);
        let mfa: Mfa = "m:f/0".parse().unwrap();
        let rows = compute_introduced_rows(&walk, &[mfa], false);
        let expected_first = bits.iter().position(|b| *b).map(tag_for);
        prop_assert_eq!(rows[0].first_tag.clone(), expected_first);
    }

    #[test]
    fn last_tag_is_the_highest_index_with_a_true_bit(
        bits in prop::collection::vec(any::<bool>(), 1..32)
    ) {
        let walk = build_walk(&bits);
        let mfa: Mfa = "m:f/0".parse().unwrap();
        let rows = compute_introduced_rows(&walk, &[mfa], false);
        let expected_last = bits.iter().rposition(|b| *b).map(tag_for);
        prop_assert_eq!(rows[0].last_tag.clone(), expected_last);
    }

    #[test]
    fn timeline_length_equals_walk_length_and_preserves_bits(
        bits in prop::collection::vec(any::<bool>(), 0..32)
    ) {
        let walk = build_walk(&bits);
        let mfa: Mfa = "m:f/0".parse().unwrap();
        let rows = compute_introduced_rows(&walk, &[mfa], true);
        let timeline = rows[0].timeline.as_ref().unwrap();
        prop_assert_eq!(timeline.len(), bits.len());
        for (entry, expected) in timeline.iter().zip(bits.iter()) {
            prop_assert_eq!(entry.present, *expected);
        }
    }

    #[test]
    fn endpoint_commits_match_corresponding_walk_entry(
        bits in prop::collection::vec(any::<bool>(), 1..32)
    ) {
        let walk = build_walk(&bits);
        let mfa: Mfa = "m:f/0".parse().unwrap();
        let rows = compute_introduced_rows(&walk, &[mfa], false);
        if let Some(first_pos) = bits.iter().position(|b| *b) {
            prop_assert_eq!(rows[0].first_commit.clone(), Some(walk[first_pos].commit.clone()));
        }
        if let Some(last_pos) = bits.iter().rposition(|b| *b) {
            prop_assert_eq!(rows[0].last_commit.clone(), Some(walk[last_pos].commit.clone()));
        }
    }

    #[test]
    fn adding_a_never_present_mfa_leaves_the_other_row_unchanged(
        bits in prop::collection::vec(any::<bool>(), 0..16)
    ) {
        let walk_single: Vec<TagSnapshot> = bits
            .iter()
            .enumerate()
            .map(|(i, &p)| TagSnapshot {
                tag: tag_for(i),
                commit: sha((i % 250) as u8),
                presence: vec![p],
            })
            .collect();
        let walk_both: Vec<TagSnapshot> = bits
            .iter()
            .enumerate()
            .map(|(i, &p)| TagSnapshot {
                tag: tag_for(i),
                commit: sha((i % 250) as u8),
                presence: vec![p, false],
            })
            .collect();
        let mfa_a: Mfa = "a:one/0".parse().unwrap();
        let mfa_b: Mfa = "b:absent/0".parse().unwrap();

        let single = compute_introduced_rows(&walk_single, &[mfa_a.clone()], false);
        let both = compute_introduced_rows(&walk_both, &[mfa_a, mfa_b], false);

        prop_assert_eq!(single[0].first_tag.clone(), both[0].first_tag.clone());
        prop_assert_eq!(single[0].last_tag.clone(), both[0].last_tag.clone());
        prop_assert_eq!(single[0].tags_present, both[0].tags_present);
        prop_assert_eq!(both[1].tags_present, 0);
        prop_assert!(both[1].first_tag.is_none());
    }
}
