// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `AttrCtxScanner` inside a `-record(...)` attribute: splits each
//! field's `::` type annotation from its default expression, so the
//! qualified-call axis can mask the annotation out and the type axis
//! can read it separately. HF-45's record addendum: a field's `::`
//! annotation read as a call into the same name as the field, because
//! the whole line scanned as body text.

use backhopper_core::compat::added_lines::added_lines_with_context;
use backhopper_core::compat::call_sites::AttrCtxScanner;
use backhopper_core::compat::patch::{Hunk, HunkLine};
use backhopper_core::model::symbol::RefContext;

/// Feeds `-record(r, {` through `s`, establishing the field-list depth
/// every subsequent line in a test builds on.
fn open_record(s: &mut AttrCtxScanner) {
    let opener = s.classify("-record(r, {");
    assert_eq!(opener.context, RefContext::RecordAttribute);
    assert!(opener.type_spans.is_empty(), "{:?}", opener.type_spans);
}

/// The substrings `line` slices to for each of `spans`.
fn span_texts<'a>(line: &'a str, spans: &[std::ops::Range<usize>]) -> Vec<&'a str> {
    spans.iter().map(|r| &line[r.clone()]).collect()
}

#[test]
fn a_field_annotation_is_a_type_span() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    vhost :: rabbit_types:vhost(),";
    let class = s.classify(line);
    assert_eq!(class.context, RefContext::RecordAttribute);
    assert_eq!(
        span_texts(line, &class.type_spans),
        [" rabbit_types:vhost()"]
    );
}

#[test]
fn a_default_before_the_annotation_stays_call_context() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    timeout = rabbit_misc:get_env(a, b, 1) :: timeout(),";
    let class = s.classify(line);
    assert_eq!(span_texts(line, &class.type_spans), [" timeout()"]);
}

#[test]
fn an_annotation_wrapped_across_lines_spans_both() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let first = "    x :: option(a,";
    let first_class = s.classify(first);
    assert_eq!(span_texts(first, &first_class.type_spans), [" option(a,"]);
    let second = "          b),";
    let second_class = s.classify(second);
    assert_eq!(second_class.context, RefContext::RecordAttribute);
    assert_eq!(second_class.type_spans, vec![0.."          b)".len()]);
}

#[test]
fn a_comma_inside_a_union_does_not_end_the_span() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    x :: option(a, b),";
    let class = s.classify(line);
    assert_eq!(span_texts(line, &class.type_spans), [" option(a, b)"]);
}

#[test]
fn a_string_default_containing_colons_is_not_a_split_point() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    name = \"a::b\" :: string(),";
    let class = s.classify(line);
    assert_eq!(span_texts(line, &class.type_spans), [" string()"]);
}

#[test]
fn a_binary_type_annotation_stays_one_span() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    payload :: <<_:_*8>>,";
    let class = s.classify(line);
    assert_eq!(span_texts(line, &class.type_spans), [" <<_:_*8>>"]);
}

#[test]
fn the_closing_brace_ends_the_last_span() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let first = "    n :: non_neg_integer()";
    let first_class = s.classify(first);
    assert_eq!(
        span_texts(first, &first_class.type_spans),
        [" non_neg_integer()"]
    );
    let second = "}).";
    let second_class = s.classify(second);
    assert!(
        second_class.type_spans.is_empty(),
        "{:?}",
        second_class.type_spans
    );
    // the region itself closes: a following line is body again
    assert_eq!(s.classify("start() -> ok.").context, RefContext::Body);
}

#[test]
fn a_record_on_one_line_classifies_and_splits() {
    let mut s = AttrCtxScanner::new();
    let line = "-record(r, {a :: t()}).";
    let class = s.classify(line);
    assert_eq!(class.context, RefContext::RecordAttribute);
    assert_eq!(span_texts(line, &class.type_spans), [" t()"]);
    assert_eq!(s.classify("start() -> ok.").context, RefContext::Body);
}

#[test]
fn a_comma_inside_a_tuple_default_is_not_a_field_separator() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    a = {1, 2} :: t(),";
    let class = s.classify(line);
    assert_eq!(span_texts(line, &class.type_spans), [" t()"]);
}

#[test]
fn a_field_with_no_annotation_has_no_spans() {
    let mut s = AttrCtxScanner::new();
    open_record(&mut s);
    let line = "    count = 0,";
    let class = s.classify(line);
    assert_eq!(class.context, RefContext::RecordAttribute);
    assert!(class.type_spans.is_empty(), "{:?}", class.type_spans);
}

#[test]
fn a_trailing_comment_does_not_shift_span_offsets() {
    let mut with_comment = AttrCtxScanner::new();
    open_record(&mut with_comment);
    let commented = "    vhost :: rabbit_types:vhost(), % scope";
    let commented_class = with_comment.classify(commented);

    let mut without_comment = AttrCtxScanner::new();
    open_record(&mut without_comment);
    let plain = "    vhost :: rabbit_types:vhost(),";
    let plain_class = without_comment.classify(plain);

    assert_eq!(commented_class.type_spans, plain_class.type_spans);
}

// The HF-45 record shape: the field list opener is a `Context` hunk
// line, only the continuation is added. Mirrors the `-spec` shape 055
// already fixed, extended to the record case: the opener's field-list
// depth must still be established even though it never enters the blob.
#[test]
fn an_orphaned_record_continuation_with_a_context_opener_splits() {
    let hunk = Hunk {
        old_start: 20,
        old_count: 1,
        new_start: 20,
        new_count: 1,
        lines: vec![
            HunkLine::Context("-record(state, {".into()),
            HunkLine::Added("    vhost :: rabbit_types:vhost(),".into()),
            HunkLine::Context("    n :: non_neg_integer()".into()),
            HunkLine::Context("}).".into()),
        ],
    };
    let (added, line_map, ctx) = added_lines_with_context(std::slice::from_ref(&hunk));
    assert_eq!(added, "    vhost :: rabbit_types:vhost(),\n");
    assert_eq!(line_map, vec![21]);
    assert_eq!(ctx.len(), 1);
    assert_eq!(ctx[0].context, RefContext::RecordAttribute);
    assert_eq!(
        span_texts(&added, &ctx[0].type_spans),
        [" rabbit_types:vhost()"]
    );
}
