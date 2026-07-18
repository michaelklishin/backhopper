// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use backhopper_driver::CancellationToken;

#[test]
fn token_starts_uncancelled() {
    let (token, _aborter) = CancellationToken::new();
    assert!(!token.is_cancelled());
}

#[test]
fn abort_flips_the_flag() {
    let (token, aborter) = CancellationToken::new();
    aborter.abort();
    assert!(token.is_cancelled());
}

#[test]
fn abort_is_idempotent() {
    let (token, aborter) = CancellationToken::new();
    aborter.abort();
    aborter.abort();
    assert!(token.is_cancelled());
}

#[test]
fn clones_share_the_same_flag() {
    let (token, aborter) = CancellationToken::new();
    let other = token.clone();
    aborter.abort();
    assert!(token.is_cancelled());
    assert!(other.is_cancelled());
}

#[test]
fn wait_returns_true_immediately_when_already_cancelled() {
    let (token, aborter) = CancellationToken::new();
    aborter.abort();
    assert!(token.wait(Some(Duration::from_millis(500))));
}

#[test]
fn wait_times_out_when_never_aborted() {
    let (token, _aborter) = CancellationToken::new();
    let start = Instant::now();
    assert!(!token.wait(Some(Duration::from_millis(50))));
    assert!(start.elapsed() >= Duration::from_millis(40));
}

#[test]
fn wait_unblocks_when_another_thread_aborts() {
    let (token, aborter) = CancellationToken::new();
    let aborter = Arc::new(aborter);
    let trip = Arc::clone(&aborter);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        trip.abort();
    });
    assert!(token.wait(Some(Duration::from_secs(1))));
}
