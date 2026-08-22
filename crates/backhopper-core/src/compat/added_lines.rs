// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The added lines of a patch's hunks as one blob plus a map from blob
//! line to file line. The target-axis resolvers scan the blob and
//! report a blob line; the map turns it into the file line the reason
//! shows.

use crate::compat::call_sites::{AttrCtxScanner, strip_line_comment};
use crate::compat::patch::{Hunk, HunkLine};
use crate::model::names::RelativePath;
use crate::model::symbol::LineClass;

/// One touched source file projected for the target-axis resolvers: its
/// source-relative path, the text of its added lines where new symbols
/// appear, and the blob-line to file-line map for the reported line.
#[derive(Debug, Clone, Copy)]
pub struct AddedLinesSubject<'a> {
    pub source_path: &'a RelativePath,
    pub added_text: &'a str,
    pub line_map: &'a [u32],
}

/// Added lines joined into one `\n`-terminated blob, paired with a map
/// from blob line (1-based) to new-file line, derived from each hunk's
/// `new_start`. A removed line does not advance the new-file counter;
/// a context line does but is not added to the blob.
pub fn added_lines_with_offsets(hunks: &[Hunk]) -> (String, Vec<u32>) {
    let mut blob = String::new();
    let mut line_map = Vec::new();
    for hunk in hunks {
        let mut new_line = hunk.new_start as u32;
        for line in &hunk.lines {
            match line {
                HunkLine::Added(s) => {
                    blob.push_str(s);
                    blob.push('\n');
                    line_map.push(new_line);
                    new_line += 1;
                }
                HunkLine::Context(_) => new_line += 1,
                HunkLine::Removed(_) => {}
            }
        }
    }
    (blob, line_map)
}

/// Added-line text and line map, like `added_lines_with_offsets`, plus
/// each blob line's attribute-region classification. The classifier
/// scans every hunk line, `Context` included, so a continuation line
/// whose multi-line `-spec`, `-callback`, `-type`, or `-opaque` opener
/// is unchanged (and so absent from the blob) still classifies against
/// the region its unseen opener started. Only `Added` lines push into
/// the blob, the line map, and the context vector; a `Context` line
/// advances the classifier and contributes nothing else.
pub fn added_lines_with_context(hunks: &[Hunk]) -> (String, Vec<u32>, Vec<LineClass>) {
    let mut blob = String::new();
    let mut line_map = Vec::new();
    let mut ctx = Vec::new();
    let mut scanner = AttrCtxScanner::new();
    for hunk in hunks {
        let mut new_line = hunk.new_start as u32;
        for line in &hunk.lines {
            match line {
                HunkLine::Added(s) => {
                    ctx.push(scanner.classify(strip_line_comment(s)));
                    blob.push_str(s);
                    blob.push('\n');
                    line_map.push(new_line);
                    new_line += 1;
                }
                HunkLine::Context(s) => {
                    scanner.classify(strip_line_comment(s));
                    new_line += 1;
                }
                HunkLine::Removed(_) => {}
            }
        }
    }
    (blob, line_map, ctx)
}

/// Translate a 1-based blob line to its file line. An empty or
/// too-short map yields the blob line unchanged, so a caller that does
/// not thread offsets gets the old blob-relative behavior.
pub fn file_line(line_map: &[u32], blob_line: u32) -> u32 {
    line_map
        .get(blob_line.saturating_sub(1) as usize)
        .copied()
        .unwrap_or(blob_line)
}
