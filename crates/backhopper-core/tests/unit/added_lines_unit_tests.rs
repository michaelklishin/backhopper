// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `added_lines_with_offsets` and `file_line`: the blob plus the
//! blob-line to file-line map the target-axis resolvers report through.
//! `added_lines_with_context`: the same projection plus each added
//! line's attribute-region classification, walked against the full
//! hunk (HF-45, 055).

use backhopper_core::compat::added_lines::{
    added_lines_with_context, added_lines_with_offsets, file_line,
};
use backhopper_core::compat::patch::{Hunk, HunkLine};
use backhopper_core::model::symbol::RefContext;

fn hunk(new_start: usize, lines: Vec<HunkLine>) -> Hunk {
    Hunk {
        old_start: 1,
        old_count: 0,
        new_start,
        new_count: lines.len(),
        lines,
    }
}

#[test]
fn added_lines_carry_their_file_line() {
    let h = hunk(
        120,
        vec![
            HunkLine::Context("ctx".into()),
            HunkLine::Added("first".into()),
            HunkLine::Added("second".into()),
        ],
    );
    let (blob, map) = added_lines_with_offsets(&[h]);
    assert_eq!(blob, "first\nsecond\n");
    assert_eq!(map, vec![121, 122]);
}

// A removed line does not advance the new-file counter; a context line
// does but is not added to the blob.
#[test]
fn removed_lines_do_not_advance_the_file_line() {
    let h = hunk(
        10,
        vec![
            HunkLine::Removed("gone".into()),
            HunkLine::Added("kept".into()),
        ],
    );
    let (blob, map) = added_lines_with_offsets(&[h]);
    assert_eq!(blob, "kept\n");
    assert_eq!(map, vec![10]);
}

#[test]
fn offsets_span_multiple_hunks() {
    let a = hunk(5, vec![HunkLine::Added("a".into())]);
    let b = hunk(40, vec![HunkLine::Added("b".into())]);
    let (_, map) = added_lines_with_offsets(&[a, b]);
    assert_eq!(map, vec![5, 40]);
}

#[test]
fn file_line_translates_through_the_map() {
    let map = [121, 122, 123];
    assert_eq!(file_line(&map, 1), 121);
    assert_eq!(file_line(&map, 3), 123);
}

// An empty or too-short map yields the blob line unchanged, so a caller
// that does not thread offsets keeps the old blob-relative behavior.
#[test]
fn file_line_falls_back_to_the_blob_line() {
    assert_eq!(file_line(&[], 7), 7);
    assert_eq!(file_line(&[121], 5), 5);
}

// HF-45: a hunk's `-spec` opener is unchanged context, only its
// continuation is added. The opener must still seed the classifier so
// the continuation reads as a type-attribute region, not a call.
#[test]
fn a_continuation_line_classifies_against_its_context_opener() {
    let h = hunk(
        118,
        vec![
            HunkLine::Context("-spec init(ConnectPacket :: mqtt_packet(),".into()),
            HunkLine::Removed("           RawSocket :: rabbit_net:socket()) -> ok.".into()),
            HunkLine::Added(
                "           Socket :: rabbit_net:socket() | rabbit_net:proxy_socket()) -> ok."
                    .into(),
            ),
        ],
    );
    let (blob, map, ctx) = added_lines_with_context(&[h]);
    assert_eq!(
        blob,
        "           Socket :: rabbit_net:socket() | rabbit_net:proxy_socket()) -> ok.\n"
    );
    assert_eq!(map, vec![119]);
    assert_eq!(ctx, vec![RefContext::TypeAttribute]);
}

// Negative control: a wrapped function call, not an attribute, whose
// opener is also unchanged context. The continuation must stay `Body`.
#[test]
fn a_body_continuation_after_a_context_call_opener_stays_body() {
    let h = hunk(
        10,
        vec![
            HunkLine::Context("f(V) -> rabbit_misc:queue_resource(V,".into()),
            HunkLine::Added("    Q).".into()),
        ],
    );
    let (_, _, ctx) = added_lines_with_context(&[h]);
    assert_eq!(ctx, vec![RefContext::Body]);
}

// No `Added` lines still advances the scanner through the hunk's
// context, but there is nothing to emit.
#[test]
fn a_context_only_hunk_produces_an_empty_blob() {
    let h = hunk(1, vec![HunkLine::Context("-spec f() -> ok.".into())]);
    let (blob, map, ctx) = added_lines_with_context(&[h]);
    assert!(blob.is_empty());
    assert!(map.is_empty());
    assert!(ctx.is_empty());
}

// The scanner carries `in_attr` across hunks in file order: an unclosed
// `-spec` in the first hunk keeps its closing continuation in the
// second hunk classified as a type attribute.
#[test]
fn classification_persists_across_hunks_in_the_same_file() {
    let a = hunk(1, vec![HunkLine::Added("-spec f(X) -> ok when".into())]);
    let b = hunk(90, vec![HunkLine::Added("      X :: othermod:t().".into())]);
    let (_, _, ctx) = added_lines_with_context(&[a, b]);
    assert_eq!(
        ctx,
        vec![RefContext::TypeAttribute, RefContext::TypeAttribute]
    );
}
