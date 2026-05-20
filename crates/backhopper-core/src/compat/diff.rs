// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Unified-diff parser. Produces `PatchedFile` instances; no semantic
//! analysis here: the caller drives the `Patch<S>` pipeline.

use std::path::PathBuf;
use std::str;

use crate::compat::patch::{Hunk, HunkLine, Language, PatchedFile};
use crate::errors::PatchError;

pub const PATCH_SIZE_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) fn parse(input: &[u8]) -> Result<Vec<PatchedFile>, PatchError> {
    if input.len() > PATCH_SIZE_LIMIT {
        return Err(PatchError::SizeLimit {
            size: input.len(),
            limit: PATCH_SIZE_LIMIT,
        });
    }
    let text = str::from_utf8(input).map_err(|e| PatchError::InvalidUtf8 {
        offset: e.valid_up_to(),
    })?;
    parse_unified_diff(text)
}

fn parse_unified_diff(text: &str) -> Result<Vec<PatchedFile>, PatchError> {
    let mut files: Vec<PatchedFile> = Vec::new();
    let mut current: Option<PatchedFile> = None;
    let mut current_hunk: Option<Hunk> = None;
    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        if line.starts_with("diff --git ") {
            if let Some(mut f) = current.take() {
                if let Some(h) = current_hunk.take() {
                    f.hunks.push(h);
                }
                files.push(f);
            }
            current = Some(PatchedFile {
                old_path: None,
                new_path: None,
                language: Language::Other,
                binary: false,
                hunks: Vec::new(),
            });
        } else if line.starts_with("--- ") {
            if let Some(f) = current.as_mut() {
                f.old_path = parse_diff_path(&line[4..]);
            }
        } else if line.starts_with("+++ ") {
            if let Some(f) = current.as_mut() {
                let path = parse_diff_path(&line[4..]);
                if let Some(p) = &path {
                    f.language = Language::from_path(&p.to_string_lossy());
                }
                f.new_path = path;
            }
        } else if line.starts_with("Binary files") || line.contains("GIT binary patch") {
            if let Some(f) = current.as_mut() {
                f.binary = true;
            }
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(f) = current.as_mut() {
                if let Some(h) = current_hunk.take() {
                    f.hunks.push(h);
                }
                let header = parse_hunk_header(rest, lineno)?;
                current_hunk = Some(header);
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            push_line(&mut current_hunk, HunkLine::Added(line[1..].to_owned()));
        } else if line.starts_with('-') && !line.starts_with("---") {
            push_line(&mut current_hunk, HunkLine::Removed(line[1..].to_owned()));
        } else if line.starts_with(' ') {
            push_line(&mut current_hunk, HunkLine::Context(line[1..].to_owned()));
        } else if line.is_empty() {
            push_line(&mut current_hunk, HunkLine::Context(String::new()));
        }
    }
    if let Some(mut f) = current.take() {
        if let Some(h) = current_hunk.take() {
            f.hunks.push(h);
        }
        files.push(f);
    }
    Ok(files)
}

fn push_line(hunk: &mut Option<Hunk>, line: HunkLine) {
    if let Some(h) = hunk.as_mut() {
        h.lines.push(line);
    }
}

fn parse_diff_path(s: &str) -> Option<PathBuf> {
    let trimmed = s.trim();
    if trimmed == "/dev/null" {
        return None;
    }
    let stripped = trimmed
        .strip_prefix("a/")
        .or_else(|| trimmed.strip_prefix("b/"))
        .unwrap_or(trimmed);
    let no_ts = stripped.split('\t').next().unwrap_or(stripped);
    Some(PathBuf::from(no_ts))
}

fn parse_hunk_header(rest: &str, lineno: usize) -> Result<Hunk, PatchError> {
    let mut parts = rest.split(' ');
    let old = parts.next().ok_or_else(|| PatchError::Malformed {
        line: lineno,
        detail: "expected old range in hunk header".into(),
    })?;
    let new = parts.next().ok_or_else(|| PatchError::Malformed {
        line: lineno,
        detail: "expected new range in hunk header".into(),
    })?;
    let (old_start, old_count) = parse_range(old.trim_start_matches('-'), lineno)?;
    let (new_start, new_count) = parse_range(new.trim_start_matches('+'), lineno)?;
    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(s: &str, lineno: usize) -> Result<(usize, usize), PatchError> {
    let (start, count) = match s.split_once(',') {
        Some((a, b)) => (a, b),
        None => (s, "1"),
    };
    let start = start.parse::<usize>().map_err(|_| PatchError::Malformed {
        line: lineno,
        detail: format!("malformed start in {s:?}"),
    })?;
    let count = count.parse::<usize>().map_err(|_| PatchError::Malformed {
        line: lineno,
        detail: format!("malformed count in {s:?}"),
    })?;
    Ok((start, count))
}
