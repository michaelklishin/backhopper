// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Preimage/postimage hunk matching for the apply axis.

use std::path::Path;

use crate::compat::evaluate::PostimageTally;
use crate::compat::patch::{Hunk, HunkLine};

pub(crate) fn preimage_lines(hunk: &Hunk) -> Vec<&str> {
    hunk.lines
        .iter()
        .filter_map(|l| match l {
            HunkLine::Context(t) | HunkLine::Removed(t) => Some(t.as_str()),
            HunkLine::Added(_) => None,
        })
        .collect()
}

pub(crate) fn postimage_lines(hunk: &Hunk) -> Vec<&str> {
    hunk.lines
        .iter()
        .filter_map(|l| match l {
            HunkLine::Context(t) | HunkLine::Added(t) => Some(t.as_str()),
            HunkLine::Removed(_) => None,
        })
        .collect()
}

pub(crate) fn leading_added_run(hunk: &Hunk) -> Vec<&str> {
    hunk.lines
        .iter()
        .map_while(|l| match l {
            HunkLine::Added(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

pub(crate) fn trailing_added_run(hunk: &Hunk) -> Vec<&str> {
    let mut run: Vec<&str> = hunk
        .lines
        .iter()
        .rev()
        .map_while(|l| match l {
            HunkLine::Added(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    run.reverse();
    run
}

pub(crate) fn preimage_offset(
    preimage: &[&str],
    old_start: usize,
    target: &[&str],
) -> Option<usize> {
    let expected = old_start.saturating_sub(1);
    if matches_at(preimage, target, expected) {
        return Some(expected);
    }
    find_subsequence(preimage, target)
}

/// The dual of `classify_preimage`: does the hunk's post-image
/// (Context plus Added lines) already exist in the pin's file? Only
/// hunks that add lines are considered; deletions get no signal.
pub(crate) fn tally_postimage(
    hunk: &Hunk,
    pre: &PreimageMatch,
    target_lines: &[&str],
    path: &Path,
    tally: &mut PostimageTally,
) {
    let added = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Added(_)))
        .count();
    if added == 0 {
        return;
    }
    let postimage = postimage_lines(hunk);
    let context = hunk
        .lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Context(_)))
        .count();
    let preimage_empty = hunk.lines.iter().all(|l| matches!(l, HunkLine::Added(_)));
    tally.considered += 1;
    let entry = tally.per_file.entry(path.to_path_buf()).or_default();
    entry.considered += 1;
    if find_subsequence(&postimage, target_lines).is_none() {
        return;
    }
    // An empty preimage classifies `Exact`, so added files must be
    // decided by the postimage alone.
    let pre_matches = !preimage_empty && !matches!(pre, PreimageMatch::Missing { .. });
    if pre_matches {
        tally.ambiguous += 1;
        entry.ambiguous += 1;
        return;
    }
    // Context lines make the postimage block a distinctive needle; a
    // context-less near-empty block is too weak to call applied.
    if context == 0 && postimage.len() < 2 {
        tally.low_confidence += 1;
        entry.low_confidence += 1;
    } else {
        tally.applied += 1;
        entry.applied += 1;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreimageMatch {
    Exact,
    Drifted { line_delta: isize },
    Missing { excerpt: String },
}

/// Classify how the hunk's preimage block (Context and Removed lines)
/// matches against `target_lines`. `Exact` is the happy path (preimage
/// at `hunk.old_start - 1`); `Drifted` recovers an offset; `Missing`
/// returns up to the first three preimage lines as an excerpt.
pub(crate) fn classify_preimage(hunk: &Hunk, target_lines: &[&str]) -> PreimageMatch {
    let preimage = preimage_lines(hunk);
    if preimage.is_empty() {
        return PreimageMatch::Exact;
    }
    let expected = hunk.old_start.saturating_sub(1);
    if matches_at(&preimage, target_lines, expected) {
        return PreimageMatch::Exact;
    }
    if let Some(found) = find_subsequence(&preimage, target_lines) {
        let delta = found as isize - expected as isize;
        return PreimageMatch::Drifted { line_delta: delta };
    }
    let excerpt = preimage
        .iter()
        .take(3)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    PreimageMatch::Missing { excerpt }
}

fn matches_at(preimage: &[&str], target: &[&str], start: usize) -> bool {
    if start + preimage.len() > target.len() {
        return false;
    }
    preimage
        .iter()
        .enumerate()
        .all(|(i, line)| target[start + i] == *line)
}

fn find_subsequence(preimage: &[&str], target: &[&str]) -> Option<usize> {
    if preimage.is_empty() || preimage.len() > target.len() {
        return None;
    }
    let last = target.len() - preimage.len();
    (0..=last).find(|&i| matches_at(preimage, target, i))
}
