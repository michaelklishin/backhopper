// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `scan_list_elements`: the list-literal twin of `scan_top_level_args`.

use backhopper_erlang_scan::{ScannedList, scan_list_elements};

fn elements(after_open_bracket: &str) -> Vec<String> {
    match scan_list_elements(after_open_bracket) {
        ScannedList::Terminated { elements, .. } => {
            elements.into_iter().map(str::to_owned).collect()
        }
        other => panic!("expected a terminated list, got {other:?}"),
    }
}

#[test]
fn a_flat_list_splits_at_commas() {
    assert_eq!(
        elements("rabbit_queue_type, drain, 1]"),
        ["rabbit_queue_type", " drain", " 1"]
    );
}

#[test]
fn an_empty_list_has_no_elements() {
    assert_eq!(elements("]"), Vec::<String>::new());
}

#[test]
fn nested_structures_do_not_split() {
    assert_eq!(
        elements("{resource, <<\"/\">>, queue}, [a, b], fun(X, Y) -> {X, Y} end]"),
        [
            "{resource, <<\"/\">>, queue}",
            " [a, b]",
            " fun(X, Y) -> {X, Y} end"
        ]
    );
}

#[test]
fn strings_and_quoted_atoms_hide_separators() {
    assert_eq!(
        elements("\"a, b]\", 'c, d]', ok]"),
        ["\"a, b]\"", " 'c, d]'", " ok"]
    );
}

#[test]
fn a_char_literal_bracket_is_inert() {
    assert_eq!(elements("$], ok]"), ["$]", " ok"]);
}

#[test]
fn a_cons_tail_is_improper() {
    assert_eq!(scan_list_elements("q | Rest]"), ScannedList::ImproperTail);
}

#[test]
fn a_comprehension_is_improper() {
    assert_eq!(
        scan_list_elements("X || X <- Qs]"),
        ScannedList::ImproperTail
    );
}

#[test]
fn a_missing_close_bracket_is_unterminated() {
    assert_eq!(
        scan_list_elements("rabbit_queue_type, drain"),
        ScannedList::Unterminated
    );
}

#[test]
fn consumed_points_past_the_close_bracket() {
    let ScannedList::Terminated { consumed, .. } = scan_list_elements("a, b] ++ More") else {
        panic!("expected terminated");
    };
    assert_eq!(consumed, 5);
}
