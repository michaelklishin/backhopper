// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_git::cherry_pick_trailers;

const SHA_A: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA_B: &str = "fedcba9876543210fedcba9876543210fedcba98";

#[test]
fn message_without_trailers_yields_nothing() {
    assert!(cherry_pick_trailers("Fix a flake\n\nLong body text.\n").is_empty());
}

#[test]
fn single_trailer_is_extracted() {
    let message = format!("Fix a flake\n\n(cherry picked from commit {SHA_A})\n");
    let trailers = cherry_pick_trailers(&message);
    assert_eq!(trailers.len(), 1);
    assert_eq!(trailers[0].as_str(), SHA_A);
}

#[test]
fn a_pick_of_a_pick_carries_every_hop() {
    let message = format!(
        "Fix a flake\n\n(cherry picked from commit {SHA_A})\n(cherry picked from commit {SHA_B})\n"
    );
    let trailers = cherry_pick_trailers(&message);
    assert_eq!(trailers.len(), 2);
    assert_eq!(trailers[0].as_str(), SHA_A);
    assert_eq!(trailers[1].as_str(), SHA_B);
}

#[test]
fn uppercase_hex_is_normalised_to_lowercase() {
    let upper = SHA_A.to_uppercase();
    let message = format!("subject\n\n(cherry picked from commit {upper})\n");
    let trailers = cherry_pick_trailers(&message);
    assert_eq!(trailers.len(), 1);
    assert_eq!(trailers[0].as_str(), SHA_A);
}

#[test]
fn short_hashes_are_not_trailers() {
    let message = "subject\n\n(cherry picked from commit 0123456789abcdef)\n";
    assert!(cherry_pick_trailers(message).is_empty());
}

#[test]
fn indented_trailers_still_match() {
    let message = format!("subject\n\n    (cherry picked from commit {SHA_A})\n");
    assert_eq!(cherry_pick_trailers(&message).len(), 1);
}
