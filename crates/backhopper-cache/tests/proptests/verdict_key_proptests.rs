// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Key-canonicalization properties: equal inputs hash equal, any
//! single field flip hashes different.

use proptest::prelude::*;

use backhopper_cache::{CacheKeyInputs, EvaluationShape, PinKeyRow, TargetKeyInputs, content_hash};
use backhopper_core::model::names::CommitSha;

fn hex_sha(seed: u8) -> CommitSha {
    format!("{:02x}", seed).repeat(20).parse().unwrap()
}

fn arb_key() -> impl Strategy<Value = CacheKeyInputs> {
    (
        any::<bool>(),
        0u8..=255,
        prop::option::of(Just("stable".to_owned())),
        1usize..4,
        "[a-f0-9]{16}",
        prop::option::of((0u8..=255, "[a-f0-9]{8}")),
    )
        .prop_map(
            |(bisect, commit_seed, series, pin_count, config, target)| CacheKeyInputs {
                shape: if bisect {
                    EvaluationShape::Bisect
                } else {
                    EvaluationShape::Check
                },
                commit: hex_sha(commit_seed),
                series: series.map(|s| s.parse().unwrap()),
                pins: (0..pin_count)
                    .map(|i| PinKeyRow {
                        project: "demo".parse().unwrap(),
                        tag: format!("v1.0.{i}").parse().unwrap(),
                        resolved_tag_sha: Some(hex_sha(i as u8)),
                        snapshot_blake3: format!("snap-{i}"),
                    })
                    .collect(),
                source_pins: Vec::new(),
                target: target.map(|(seed, hash)| TargetKeyInputs {
                    resolved_commit: hex_sha(seed),
                    translations_blake3: hash,
                    walk_limit: 5000,
                    already_present_enabled: true,
                }),
                config_blake3: config,
                schema_version: 9,
                crate_version: "0.13.0".to_owned(),
            },
        )
}

proptest! {
    #[test]
    fn equal_inputs_hash_equal(key in arb_key()) {
        prop_assert_eq!(content_hash(&key).unwrap(), content_hash(&key.clone()).unwrap());
    }

    #[test]
    fn any_field_flip_hashes_different(key in arb_key(), flip in 0usize..7) {
        let mut other = key.clone();
        match flip {
            0 => other.commit = if other.commit == hex_sha(0) { hex_sha(1) } else { hex_sha(0) },
            1 => other.config_blake3.push('x'),
            2 => other.crate_version.push('x'),
            3 => other.schema_version += 1,
            4 => other.pins[0].snapshot_blake3.push('x'),
            5 => other.pins[0].resolved_tag_sha = None,
            _ => {
                other.shape = match other.shape {
                    EvaluationShape::Check => EvaluationShape::Bisect,
                    EvaluationShape::Bisect => EvaluationShape::Check,
                };
            }
        }
        prop_assert_ne!(content_hash(&key).unwrap(), content_hash(&other).unwrap());
    }

    #[test]
    fn the_content_key_carries_every_shared_component(key in arb_key(), patch in "[a-f0-9]{16}") {
        let content = key.content_key(patch.clone(), Some("env".to_owned()));
        prop_assert_eq!(content.shape, key.shape);
        prop_assert_eq!(&content.pins, &key.pins);
        prop_assert_eq!(&content.config_blake3, &key.config_blake3);
        prop_assert_eq!(&content.patch_blake3, &patch);
        // two commits with the same content components share one
        // content key: the cascade-reuse invariant
        let mut other = key.clone();
        other.commit = if other.commit == hex_sha(0) { hex_sha(1) } else { hex_sha(0) };
        let other_content = other.content_key(patch, Some("env".to_owned()));
        prop_assert_eq!(
            content_hash(&other_content).unwrap(),
            content_hash(&content).unwrap()
        );
    }
}
