// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `added_lines_with_offsets` and `file_line`: the blob plus the
//! blob-line to file-line map the target-axis resolvers report through.

use backhopper_core::compat::added_lines::{added_lines_with_offsets, file_line};
use backhopper_core::compat::patch::{Hunk, HunkLine};

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
