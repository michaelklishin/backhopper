// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The OTP 27 and 28 operators (`?=`, `<:-`, `<:=`, `&&`) and the
//! native-record wildcard `#_` must leave every scanner reading
//! unchanged: these pin the safety rather than add behavior.

use backhopper_erlang_scan::{
    ScanArity, ScannedList, scan_arity, scan_list_elements, split_top_level_args,
};

fn exact(input: &str) -> u8 {
    match scan_arity(input) {
        ScanArity::Exact(n) => n,
        ScanArity::Unterminated => panic!("unterminated: {input}"),
    }
}

#[test]
fn a_conditional_match_keeps_the_argument_count() {
    assert_eq!(exact("maybe Pid ?= khepri:whereis(Id), Pid end, Y)"), 2);
}

#[test]
fn strict_and_zip_generators_stay_inside_their_comprehension() {
    assert_eq!(exact("[{A, B} || A <:- L1 && B <:= M1], Y)"), 2);
    let twin = exact("[{A, B} || A <- L1, B <- M1], Y)");
    assert_eq!(twin, 2);
}

#[test]
fn an_assignment_qualifier_stays_inside_its_comprehension() {
    assert_eq!(exact("[B || A <- L, B = f(A)], Y)"), 2);
}

#[test]
fn the_wildcard_record_does_not_perturb_depth_or_splits() {
    assert_eq!(exact("#_{}, Y)"), 2);
    assert_eq!(
        split_top_level_args("#_{a = 1}, #_.field)"),
        vec!["#_{a = 1}", " #_.field"]
    );
}

#[test]
fn new_operators_in_a_list_keep_the_element_count() {
    match scan_list_elements("A ?= B, C && D]") {
        ScannedList::Terminated { elements, .. } => assert_eq!(elements.len(), 2),
        other => panic!("expected terminated list, got {other:?}"),
    }
}
