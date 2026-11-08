// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::iter;
use std::sync::LazyLock;

use backhopper_erlang::cond_compile::CondStack;
use backhopper_erlang_scan::{literal_span, quoted_atom_span};
use regex::Regex;

/// A source file with its comments and string contents blanked out. Both
/// views keep every byte offset and line break of the original, so an
/// offset in either one names the same line in the file.
#[derive(Debug)]
pub(crate) struct Code {
    stripped: String,
    masked: String,
    bare: String,
    line_starts: Vec<usize>,
}

fn blank(span: &[u8], out: &mut Vec<u8>) {
    out.extend(span.iter().map(|&b| if b == b'\n' { b } else { b' ' }));
}

impl Code {
    pub(crate) fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut stripped: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut masked: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut bare: Vec<u8> = Vec::with_capacity(bytes.len());
        let mut i = 0usize;
        while i < bytes.len() {
            let span = match bytes[i] {
                b'%' => {
                    let end = bytes[i..]
                        .iter()
                        .position(|&b| b == b'\n')
                        .map_or(bytes.len(), |n| i + n);
                    blank(&bytes[i..end], &mut stripped);
                    blank(&bytes[i..end], &mut masked);
                    blank(&bytes[i..end], &mut bare);
                    i = end;
                    continue;
                }
                b'"' | b'~' | b'$' => literal_span(bytes, i).unwrap_or(1),
                b'\'' => quoted_atom_span(bytes, i).unwrap_or(1),
                _ => 1,
            };
            let text = &bytes[i..i + span];
            stripped.extend_from_slice(text);
            let is_atom = bytes[i] == b'\'';
            if matches!(bytes[i], b'"' | b'~' | b'$') && span > 1 {
                blank(text, &mut masked);
            } else {
                masked.extend_from_slice(text);
            }
            if (is_atom || matches!(bytes[i], b'"' | b'~' | b'$')) && span > 1 {
                blank(text, &mut bare);
            } else {
                bare.extend_from_slice(text);
            }
            i += span;
        }
        let line_starts = iter::once(0)
            .chain(
                bytes
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b == b'\n')
                    .map(|(at, _)| at + 1),
            )
            .collect();
        Self {
            stripped: String::from_utf8_lossy(&stripped).into_owned(),
            masked: String::from_utf8_lossy(&masked).into_owned(),
            bare: String::from_utf8_lossy(&bare).into_owned(),
            line_starts,
        }
    }

    /// Comments blanked, string literals intact.
    pub(crate) fn stripped(&self) -> &str {
        &self.stripped
    }

    /// Comments blanked, string literal contents blanked too.
    pub(crate) fn masked(&self) -> &str {
        &self.masked
    }

    /// Comments, strings and quoted atoms blanked: only code is left.
    pub(crate) fn bare(&self) -> &str {
        &self.bare
    }

    /// The 1-based line of a byte offset.
    pub(crate) fn line_at(&self, offset: usize) -> u32 {
        let line = self.line_starts.partition_point(|&start| start <= offset);
        u32::try_from(line).unwrap_or(u32::MAX)
    }
}

/// What the compiler does with a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Region {
    Live,
    /// Under a conditional other than `TEST`: whether the code runs
    /// depends on the build, which is not evaluated.
    Conditional,
    /// Under `-ifdef(TEST)`: not production code.
    TestOnly,
}

static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*-\s*(ifdef|ifndef|if|elif|else|endif)\b\s*(?:\(\s*([^)]*?)\s*\))?")
        .expect("regex")
});

static DEFINE_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*-\s*define\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)").expect("regex"));

/// The region of every line. An `-ifndef(X)` whose first form defines `X`
/// is an include guard and does not make its contents conditional.
pub(crate) fn regions(code: &Code) -> Vec<Region> {
    let lines: Vec<&str> = code.masked().lines().collect();
    let mut stack = CondStack::new();
    let mut out = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        if let Some(caps) = DIRECTIVE.captures(line) {
            let argument = caps.get(2).map_or("", |m| m.as_str());
            match &caps[1] {
                "ifdef" => stack.push_ifdef(argument),
                "ifndef" => {
                    stack.push_ifndef(argument);
                    if opens_with_define(&lines[index + 1..], argument) {
                        stack.mark_include_guard();
                    }
                }
                "if" => stack.push_if(argument),
                "else" => stack.flip_else(),
                "endif" => stack.pop_endif(),
                _ => {}
            }
        }
        out.push(if stack.is_test_only() {
            Region::TestOnly
        } else if stack.under_non_test_condition() {
            Region::Conditional
        } else {
            Region::Live
        });
    }
    out
}

fn opens_with_define(rest: &[&str], name: &str) -> bool {
    rest.iter()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| DEFINE_HEAD.captures(line))
        .is_some_and(|caps| &caps[1] == name)
}

pub(crate) fn region_at(regions: &[Region], line: u32) -> Region {
    regions
        .get((line as usize).saturating_sub(1))
        .copied()
        .unwrap_or(Region::Live)
}
