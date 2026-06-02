// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_xref_reader::scanner::{Pos, Scanner};

#[test]
fn pos_start_is_one_indexed() {
    let p = Pos::start();
    assert_eq!(p.line, 1);
    assert_eq!(p.column, 1);
    assert_eq!(p.byte_offset, 0);
}

#[test]
fn advance_tracks_line_breaks() {
    let mut sc = Scanner::new("a\nb");
    assert_eq!(sc.advance(), Some(b'a'));
    assert_eq!(sc.advance(), Some(b'\n'));
    assert_eq!(sc.pos().line, 2);
    assert_eq!(sc.pos().column, 1);
    assert_eq!(sc.advance(), Some(b'b'));
    assert_eq!(sc.pos().column, 2);
}

#[test]
fn skip_trivia_advances_through_whitespace_and_comments() {
    let mut sc = Scanner::new("  % comment\n   foo");
    sc.skip_trivia();
    assert_eq!(sc.peek(), Some(b'f'));
}

#[test]
fn consume_string_skips_closing_quote() {
    let mut sc = Scanner::new(r#""hello world" rest"#);
    sc.consume_string();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn consume_string_handles_escaped_quotes() {
    let mut sc = Scanner::new(r#""a\"b" rest"#);
    sc.consume_string();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn consume_quoted_atom_skips_closing_quote() {
    let mut sc = Scanner::new("'with spaces' rest");
    sc.consume_quoted_atom();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn consume_identifier_stops_at_non_word() {
    let mut sc = Scanner::new("hello_world(");
    assert_eq!(sc.consume_identifier(), "hello_world");
    assert_eq!(sc.peek(), Some(b'('));
}

#[test]
fn consume_macro_ref_strips_question_mark() {
    let mut sc = Scanner::new("?MODULE rest");
    assert_eq!(sc.consume_macro_ref(), "MODULE");
}

#[test]
fn set_pos_supports_backtracking() {
    let mut sc = Scanner::new("abc");
    let start = sc.pos();
    sc.advance();
    sc.advance();
    sc.set_pos(start);
    assert_eq!(sc.peek(), Some(b'a'));
}

#[test]
fn char_literal_simple_skips_two_bytes() {
    let mut sc = Scanner::new("$x rest");
    sc.consume_char_literal();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn char_literal_escaped_skips_three_bytes() {
    let mut sc = Scanner::new(r#"$\n rest"#);
    sc.consume_char_literal();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn char_literal_caret_control_skips_four_bytes() {
    let mut sc = Scanner::new(r#"$\^A rest"#);
    sc.consume_char_literal();
    assert_eq!(sc.peek(), Some(b' '));
}

#[test]
fn skip_line_advances_past_newline() {
    let mut sc = Scanner::new("abc\nxyz");
    sc.skip_line();
    assert_eq!(sc.peek(), Some(b'x'));
    assert_eq!(sc.pos().line, 2);
}

#[test]
fn is_eof_true_after_all_bytes_consumed() {
    let mut sc = Scanner::new("ab");
    sc.advance();
    sc.advance();
    assert!(sc.is_eof());
    assert_eq!(sc.peek(), None);
}

#[test]
fn skip_balanced_parens_handles_close_paren_inside_string() {
    // The `)` inside the string must not close the outer paren.
    let mut sc = Scanner::new(r#"("a)b", x)trail"#);
    let commas = sc.skip_balanced_parens();
    assert_eq!(commas, 1);
    assert_eq!(sc.peek(), Some(b't'));
}

#[test]
fn skip_balanced_parens_handles_close_paren_inside_quoted_atom() {
    let mut sc = Scanner::new("('a)b', x)trail");
    let commas = sc.skip_balanced_parens();
    assert_eq!(commas, 1);
    assert_eq!(sc.peek(), Some(b't'));
}

#[test]
fn skip_balanced_parens_handles_close_paren_inside_char_literal() {
    // `$)` is the char literal for `)`; it must not close the outer paren.
    let mut sc = Scanner::new("($), x)trail");
    let commas = sc.skip_balanced_parens();
    assert_eq!(commas, 1);
    assert_eq!(sc.peek(), Some(b't'));
}

#[test]
fn skip_balanced_parens_handles_double_quote_char_literal() {
    // `$"` is the char literal for `"`. Without `$` handling, the next `"` opens a string.
    let mut sc = Scanner::new(r#"($", x)trail"#);
    let commas = sc.skip_balanced_parens();
    assert_eq!(commas, 1);
    assert_eq!(sc.peek(), Some(b't'));
}

#[test]
fn skip_balanced_parens_handles_close_paren_inside_string_in_nested_brackets() {
    let mut sc = Scanner::new(r#"([{"a)b"}], x)trail"#);
    let commas = sc.skip_balanced_parens();
    assert_eq!(commas, 1);
    assert_eq!(sc.peek(), Some(b't'));
}
