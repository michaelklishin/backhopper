// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang::cond_compile::CondStack;

#[test]
fn ifdef_test_marks_inside_test_only() {
    let mut s = CondStack::new();
    assert!(!s.is_test_only());
    s.push_ifdef("TEST");
    assert!(s.is_test_only());
    s.pop_endif();
    assert!(!s.is_test_only());
}

#[test]
fn ifdef_other_macro_does_not_mark_test_only() {
    let mut s = CondStack::new();
    s.push_ifdef("HIPE");
    assert!(!s.is_test_only());
}

#[test]
fn else_inside_ifdef_test_flips_off_test_only() {
    let mut s = CondStack::new();
    s.push_ifdef("TEST");
    assert!(s.is_test_only());
    s.flip_else();
    assert!(!s.is_test_only());
}

#[test]
fn endif_pops_state() {
    let mut s = CondStack::new();
    s.push_ifdef("TEST");
    s.pop_endif();
    assert!(!s.is_test_only());
}