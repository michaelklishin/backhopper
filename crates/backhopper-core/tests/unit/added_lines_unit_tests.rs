// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `added_lines_with_offsets` and `file_line`: the blob plus the
//! blob-line to file-line map the target-axis resolvers report through.
//! `added_lines_with_context`: the same projection plus each added
//! line's attribute-region classification, computed against the full
//! hunk.

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

// Removed lines do not advance the new-file counter; context lines do but stay out of the blob.
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

// An empty or too-short map yields the blob line unchanged: the old blob-relative behavior.
#[test]
fn file_line_falls_back_to_the_blob_line() {
    assert_eq!(file_line(&[], 7), 7);
    assert_eq!(file_line(&[121], 5), 5);
}

// The -spec opener is unchanged context; it must still seed the classifier
// so the added continuation classifies as a type attribute, not a call.
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

// Negative control: a wrapped call opener in unchanged context; the continuation must stay Body.
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

// No Added lines: the scanner still advances through context but emits nothing.
#[test]
fn a_context_only_hunk_produces_an_empty_blob() {
    let h = hunk(1, vec![HunkLine::Context("-spec f() -> ok.".into())]);
    let (blob, map, ctx) = added_lines_with_context(&[h]);
    assert!(blob.is_empty());
    assert!(map.is_empty());
    assert!(ctx.is_empty());
}

// in_attr carries across hunks in file order: an unclosed -spec in the first
// hunk keeps the second hunk's continuation classified as a type attribute.
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

#[test]
fn an_export_attribute_classifies_as_other_attribute() {
    let h = hunk(
        3,
        vec![
            HunkLine::Added("-export([info/1,".into()),
            HunkLine::Added("         format_status/2]).".into()),
            HunkLine::Added("info(S) -> S.".into()),
        ],
    );
    let (_, _, ctx) = added_lines_with_context(&[h]);
    assert_eq!(
        ctx,
        vec![
            RefContext::OtherAttribute,
            RefContext::OtherAttribute,
            RefContext::Body
        ]
    );
}

// The HF-49 hunk shape: the modified -spec head is added, the spec's
// terminating line stayed context. The terminator must still close the
// region so the clause heads after it classify as body.
#[test]
fn a_context_terminator_closes_a_region_an_added_opener_started() {
    let h = hunk(
        290,
        vec![
            HunkLine::Removed("-spec parse_props(binary(), protocol_version()) ->".into()),
            HunkLine::Added(
                "-spec parse_props(binary(), protocol_version(), packet_type()) ->".into(),
            ),
            HunkLine::Context("    {properties(), binary()}.".into()),
            HunkLine::Removed("parse_props(Bin, Vsn)".into()),
            HunkLine::Added("parse_props(Bin, Vsn, _Type)".into()),
            HunkLine::Context("  when Vsn < 5 ->".into()),
        ],
    );
    let (_, _, ctx) = added_lines_with_context(&[h]);
    assert_eq!(ctx, vec![RefContext::TypeAttribute, RefContext::Body]);
}
