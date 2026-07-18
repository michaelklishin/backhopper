// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_erlang_scan::{
    ScanArity, ScannedArgs, count_top_level_commas, count_top_level_items, scan_arity,
    scan_top_level_args, skip_char_literal_span, split_top_level_args, split_top_level_commas,
    take_balanced_parens,
};

// skip_char_literal_span

#[test]
fn char_literal_plain() {
    assert_eq!(skip_char_literal_span(b"$a", 0), 2);
    assert_eq!(skip_char_literal_span(b"$)", 0), 2);
}

#[test]
fn char_literal_escape_and_control() {
    assert_eq!(skip_char_literal_span(b"$\\n", 0), 3);
    assert_eq!(skip_char_literal_span(b"$\\^A", 0), 4);
}

#[test]
fn char_literal_truncated_spans() {
    assert_eq!(skip_char_literal_span(b"$", 0), 1);
    assert_eq!(skip_char_literal_span(b"$\\", 0), 2);
    assert_eq!(skip_char_literal_span(b"$\\^", 0), 3);
}

// scan_top_level_args

fn terminated(scanned: ScannedArgs<'_>) -> (Vec<String>, usize) {
    match scanned {
        ScannedArgs::Terminated { args, consumed } => {
            (args.iter().map(|s| s.to_string()).collect(), consumed)
        }
        ScannedArgs::Unterminated { .. } => panic!("expected Terminated"),
    }
}

#[test]
fn empty_arg_list_is_zero_arity() {
    let (args, consumed) = terminated(scan_top_level_args(")"));
    assert!(args.is_empty());
    assert_eq!(consumed, 1);
}

#[test]
fn single_arg_reports_consumed_past_close() {
    let (args, consumed) = terminated(scan_top_level_args("A)rest"));
    assert_eq!(args, vec!["A"]);
    assert_eq!(consumed, 2);
}

#[test]
fn nested_brackets_do_not_split() {
    assert_eq!(
        terminated(scan_top_level_args("[a, b], {c, d})")).0.len(),
        2
    );
}

#[test]
fn commas_in_strings_and_atoms_do_not_split() {
    assert_eq!(terminated(scan_top_level_args("\"a, b\", c)")).0.len(), 2);
    assert_eq!(terminated(scan_top_level_args("'a, b', c)")).0.len(), 2);
}

#[test]
fn char_literal_comma_and_paren_are_inert() {
    // $, is the comma char and $) the close-paren char; neither splits nor closes the list
    assert_eq!(
        terminated(scan_top_level_args("$,, X)")).0,
        vec!["$,", " X"]
    );
    assert_eq!(terminated(scan_top_level_args("$))")).0, vec!["$)"]);
}

#[test]
fn scan_args_consumes_control_char_literal_whole() {
    // $\^A is a 4-byte control-char literal consumed as one unit, so a following comma still separates
    assert_eq!(
        terminated(scan_top_level_args("$\\^A, B)")).0,
        vec!["$\\^A", " B"]
    );
}

#[test]
fn bitstring_argument_stays_one_arg() {
    assert_eq!(
        terminated(scan_top_level_args("<<_:8, _:_*8>>, Opts)"))
            .0
            .len(),
        2
    );
}

#[test]
fn unterminated_keeps_the_args_seen() {
    match scan_top_level_args("A, B") {
        ScannedArgs::Unterminated { args } => assert_eq!(args, vec!["A", " B"]),
        ScannedArgs::Terminated { .. } => panic!("expected Unterminated"),
    }
}

#[test]
fn empty_input_is_unterminated() {
    assert!(matches!(
        scan_top_level_args(""),
        ScannedArgs::Unterminated { args } if args.is_empty()
    ));
}

// scan_arity

#[test]
fn arity_zero_one_and_unterminated() {
    assert_eq!(scan_arity(")"), ScanArity::Exact(0));
    assert_eq!(scan_arity("A)"), ScanArity::Exact(1));
    assert_eq!(scan_arity("A, B, C)"), ScanArity::Exact(3));
    assert_eq!(scan_arity("A, B"), ScanArity::Unterminated);
}

// split_top_level_args

#[test]
fn split_args_is_lenient_about_missing_close() {
    assert_eq!(split_top_level_args("A, B"), vec!["A", " B"]);
    assert_eq!(split_top_level_args("A, B)"), vec!["A", " B"]);
}

// count_top_level_items

#[test]
fn item_count_handles_tuples_lists_and_nesting() {
    assert_eq!(
        count_top_level_items("{ok, ra:index(), term()}", '{', '}'),
        3
    );
    assert_eq!(count_top_level_items("{a, {b, c}}", '{', '}'), 2);
    assert_eq!(count_top_level_items("[x, y]", '[', ']'), 2);
    assert_eq!(count_top_level_items("{}", '{', '}'), 0);
    assert_eq!(count_top_level_items("no brackets", '{', '}'), 0);
}

// split_top_level_commas

#[test]
fn split_commas_basic_and_empty() {
    assert_eq!(split_top_level_commas("a, b, c"), vec!["a", "b", "c"]);
    assert!(split_top_level_commas("").is_empty());
    assert_eq!(split_top_level_commas("only"), vec!["only"]);
}

#[test]
fn split_commas_trims_and_drops_empties() {
    assert_eq!(split_top_level_commas("  a ,  , b "), vec!["a", "b"]);
    assert!(split_top_level_commas(" , , ").is_empty());
}

#[test]
fn split_commas_keeps_bitstring_field_whole() {
    // a comma inside a multi-segment bitstring field type is not a field separator
    let fields = split_top_level_commas("key, payload :: <<_:8, _:_*8>>, version");
    assert_eq!(fields, vec!["key", "payload :: <<_:8, _:_*8>>", "version"]);
}

#[test]
fn split_commas_ignores_nested_and_quoted_commas() {
    assert_eq!(
        split_top_level_commas("{a, b}, [c, d], \"e, f\", 'g, h'"),
        vec!["{a, b}", "[c, d]", "\"e, f\"", "'g, h'"]
    );
}

#[test]
fn split_commas_treats_char_literal_comma_as_data() {
    assert_eq!(split_top_level_commas("$,, x"), vec!["$,", "x"]);
}

// count_top_level_commas

#[test]
fn count_commas_excludes_nested_and_bitstring() {
    assert_eq!(count_top_level_commas("a, {b, c}, <<_:8, _:_*8>>"), 2);
    assert_eq!(count_top_level_commas("<<_:8, _:_*8>>"), 0);
    assert_eq!(count_top_level_commas(""), 0);
}

// take_balanced_parens

#[test]
fn balanced_parens_inner_and_rest() {
    assert_eq!(
        take_balanced_parens("(Id :: ra:index(), Cmd) -> ok"),
        Some(("Id :: ra:index(), Cmd", " -> ok"))
    );
}

#[test]
fn balanced_parens_empty_and_nested() {
    assert_eq!(take_balanced_parens("()"), Some(("", "")));
    assert_eq!(
        take_balanced_parens("(a, (b, c)) x"),
        Some(("a, (b, c)", " x"))
    );
}

#[test]
fn balanced_parens_ignores_parens_in_strings_and_atoms() {
    assert_eq!(take_balanced_parens("(\"a)b\") z"), Some(("\"a)b\"", " z")));
    assert_eq!(take_balanced_parens("('a)b') z"), Some(("'a)b'", " z")));
}

#[test]
fn balanced_parens_treats_char_literal_paren_as_data() {
    // $) is the close-paren char literal, not a closing paren
    assert_eq!(take_balanced_parens("($), x)"), Some(("$), x", "")));
}

#[test]
fn balanced_parens_none_when_unbalanced_or_no_open() {
    assert_eq!(take_balanced_parens("(no close"), None);
    assert_eq!(take_balanced_parens("no parens"), None);
}
