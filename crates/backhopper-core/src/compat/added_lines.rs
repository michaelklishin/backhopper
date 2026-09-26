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

/// What the attribute-region classifier reads to classify added lines.
#[derive(Debug, Clone, Copy)]
pub enum LineContextSource<'a> {
    /// The file as the patch leaves it, so a multi-line `-spec`, `-type`,
    /// or `-record` opened above the hunk still classifies its lines.
    PostImage(&'a str),
    /// The hunk lines alone, for a patch with no tree to read.
    HunksOnly,
}

/// Added-line text and line map, like `added_lines_with_offsets`, plus
/// each blob line's attribute-region classification. A post-image that
/// disagrees with an added line is not the file the hunks came from, and
/// the hunks are classified on their own instead.
pub fn added_lines_with_context(
    hunks: &[Hunk],
    source: LineContextSource<'_>,
) -> (String, Vec<u32>, Vec<LineClass>) {
    let (blob, line_map) = added_lines_with_offsets(hunks);
    let ctx = match source {
        LineContextSource::PostImage(text) => post_image_classes(text, &blob, &line_map),
        LineContextSource::HunksOnly => None,
    }
    .unwrap_or_else(|| hunk_classes(hunks));
    (blob, line_map, ctx)
}

fn post_image_classes(text: &str, blob: &str, line_map: &[u32]) -> Option<Vec<LineClass>> {
    let mut scanner = AttrCtxScanner::new();
    let classified: Vec<(&str, LineClass)> = text
        .lines()
        .map(|line| (line, scanner.classify(strip_line_comment(line))))
        .collect();
    blob.lines()
        .zip(line_map)
        .map(|(added, &file_line)| {
            let (line, class) = classified.get((file_line as usize).checked_sub(1)?)?;
            (line.trim_end() == added.trim_end()).then(|| class.clone())
        })
        .collect()
}

/// The classifier walks every hunk line, `Context` included, so a
/// continuation line whose opener is inside the hunk still classifies
/// against it. Only `Added` lines produce a class.
fn hunk_classes(hunks: &[Hunk]) -> Vec<LineClass> {
    let mut ctx = Vec::new();
    let mut scanner = AttrCtxScanner::new();
    for hunk in hunks {
        for line in &hunk.lines {
            match line {
                HunkLine::Added(s) => ctx.push(scanner.classify(strip_line_comment(s))),
                HunkLine::Context(s) => {
                    scanner.classify(strip_line_comment(s));
                }
                HunkLine::Removed(_) => {}
            }
        }
    }
    ctx
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
