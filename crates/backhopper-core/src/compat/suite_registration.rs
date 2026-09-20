// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! C5: a new test suite added to an app whose Makefile dispatches
//! suites by explicit registration (a family-declared marker) must be
//! referenced from that Makefile. `read_target` is injected: core
//! stays I/O-free.

use std::collections::BTreeMap;
use std::path::Path;

use crate::compat::added_lines::added_lines_with_offsets;
use crate::compat::patch::PatchedFile;
use crate::model::names::RelativePath;
use crate::model::verdict::Reason;

/// Flag each added `_SUITE.erl` whose owning app's target Makefile
/// carries `marker` as a line-anchored assignment and does not name
/// the suite's base name on either the target Makefile or this
/// patch's own added Makefile lines.
pub fn analyse_suite_registration(
    files: &[PatchedFile],
    marker: &str,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<Reason> {
    let makefile_added = collect_makefile_added_lines(files);
    let mut reasons = Vec::new();
    for file in files {
        // Only added suites: registration already existed, or never
        // did, for a modified one; the round did not change it.
        if file.old_path.is_some() || file.binary {
            continue;
        }
        let Some(new_path) = file.new_path.as_deref() else {
            continue;
        };
        if new_path == Path::new("/dev/null") {
            continue;
        }
        let Some(suite_path) = new_path.to_str().and_then(|s| RelativePath::new(s).ok()) else {
            continue;
        };
        let Some((app_dir, base_name)) = suite_app_and_base(&suite_path) else {
            continue;
        };
        let Some(makefile_path) = makefile_path_for(&app_dir) else {
            continue;
        };
        let target_content = read_target(&makefile_path).unwrap_or_default();
        if !makefile_has_marker(&target_content, marker) {
            // No marker: this app dispatches suites by discovery.
            continue;
        }
        let added_here = makefile_added
            .get(&makefile_path)
            .map(String::as_str)
            .unwrap_or("");
        if word_boundary_contains(&target_content, &base_name)
            || word_boundary_contains(added_here, &base_name)
        {
            continue;
        }
        reasons.push(Reason::SuiteNotRegisteredForCt {
            suite_path,
            makefile_path,
        });
    }
    reasons
}

/// Added-line text of every touched Makefile in this same patch,
/// keyed by path: a patch that both adds a suite and registers it in
/// the same commit must not fire.
fn collect_makefile_added_lines(files: &[PatchedFile]) -> BTreeMap<RelativePath, String> {
    let mut out = BTreeMap::new();
    for file in files {
        if file.binary {
            continue;
        }
        let Some(new_path) = file.new_path.as_deref() else {
            continue;
        };
        if new_path.file_name().and_then(|n| n.to_str()) != Some("Makefile") {
            continue;
        }
        let Some(path) = new_path.to_str().and_then(|s| RelativePath::new(s).ok()) else {
            continue;
        };
        let (added, _) = added_lines_with_offsets(&file.hunks);
        if !added.is_empty() {
            out.insert(path, added);
        }
    }
    out
}

/// A path whose last two segments are `test/<name>_SUITE.erl`: the app
/// directory (everything before `test/`) and the suite base name
/// (`<name>`, the registration lines' own vocabulary). `None` for a
/// nested `test/sub/<name>_SUITE.erl`: only direct children of `test/`
/// are CT-discovered suites in v1.
fn suite_app_and_base(path: &RelativePath) -> Option<(String, String)> {
    let parts: Vec<&str> = path.as_str().split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    let file_name = parts[parts.len() - 1];
    let parent = parts[parts.len() - 2];
    if parent != "test" {
        return None;
    }
    let base_name = file_name.strip_suffix("_SUITE.erl")?;
    if base_name.is_empty() {
        return None;
    }
    // Empty for a single-app project (`test/` at the repo root): the
    // Makefile then sits at the repo root too.
    let app_dir = parts[..parts.len() - 2].join("/");
    Some((app_dir, base_name.to_owned()))
}

/// `<app_dir>/Makefile`, or bare `Makefile` when `app_dir` is the
/// repo root.
fn makefile_path_for(app_dir: &str) -> Option<RelativePath> {
    if app_dir.is_empty() {
        RelativePath::new("Makefile").ok()
    } else {
        RelativePath::new(format!("{app_dir}/Makefile")).ok()
    }
}

/// True when some non-comment line's first non-blank token starts
/// with `marker`: a Makefile comment mentioning the marker must not
/// bring the app under the registration rule.
fn makefile_has_marker(content: &str, marker: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with('#') && trimmed.starts_with(marker)
    })
}

/// Whether `word` occurs in `text` bounded by non-identifier bytes on
/// both sides: `cluster` registered must not satisfy an added
/// `cluster_minority_SUITE.erl`, nor the reverse.
pub(crate) fn word_boundary_contains(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let mut search_from = 0usize;
    while let Some(offset) = text[search_from..].find(word) {
        let start = search_from + offset;
        let end = start + word.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        search_from = start + 1;
        if search_from >= text.len() {
            break;
        }
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
