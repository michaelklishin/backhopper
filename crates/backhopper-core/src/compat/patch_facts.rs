// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Source-side classifiers that populate `PatchFacts`. Pure functions over
//! `PatchedFile` slices: per-file logging style, Khepri-touch signals,
//! versioned-record introductions. Strictly separate from the verdict
//! pipeline: nothing here ever becomes a `Reason`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::str::FromStr;

use crate::compat::patch::{HunkLine, Language, PatchedFile};
use crate::model::names::RecordName;
use crate::model::verdict::{FileLoggingStyle, KhepriSignals, LoggingStyle, PatchFacts};

/// Heuristic suite suggestions derived from touched paths. Emits
/// `<plugin>_config_schema_SUITE` for each touched `.schema` or
/// `.snippets` file under a `deps/<plugin>/priv/schema/` path. Never
/// short-circuited by the verdict promoter: even an `Inapplicable`
/// schema-only diff carries these suggestions in `Diagnostics`.
pub(crate) fn suggest_suites(files: &[PatchedFile]) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let Some(path) = file.new_path.as_ref().or(file.old_path.as_ref()) else {
            continue;
        };
        let path_str = path.to_string_lossy();
        let lower = path_str.to_ascii_lowercase();
        if !(lower.ends_with(".schema") || lower.ends_with(".snippets")) {
            continue;
        }
        if let Some(plugin) = infer_plugin_from_schema_path(&lower) {
            out.insert(format!("{plugin}_config_schema_SUITE"));
        }
    }
    out.into_iter().collect()
}

fn infer_plugin_from_schema_path(lower_path: &str) -> Option<String> {
    let mut segments = lower_path.split('/');
    while let Some(seg) = segments.next() {
        if seg == "deps" {
            if let Some(plugin) = segments.next() {
                return Some(plugin.to_owned());
            }
        }
    }
    None
}

pub(crate) fn classify(files: &[PatchedFile]) -> PatchFacts {
    let mut logging_style: BTreeMap<PathBuf, FileLoggingStyle> = BTreeMap::new();
    let mut touches_khepri_module = false;
    let mut uses_khepri_macros = false;
    let mut saw_mnesia_branch = false;
    let mut saw_khepri_branch = false;
    let mut introduces_versioned_record: BTreeSet<RecordName> = BTreeSet::new();
    for file in files {
        let Some(path) = file.new_path.as_ref().or(file.old_path.as_ref()) else {
            continue;
        };
        let path_str = path.to_string_lossy();
        let path_lower = path_str.to_ascii_lowercase();
        if file_basename_matches_khepri(&path_lower) {
            touches_khepri_module = true;
        }
        let mut logger_macro_sites = 0usize;
        let mut rabbit_log_sites = 0usize;
        for hunk in &file.hunks {
            for line in &hunk.lines {
                let text = match line {
                    HunkLine::Added(t) | HunkLine::Context(t) => t.as_str(),
                    HunkLine::Removed(_) => continue,
                };
                logger_macro_sites += count_logger_macro(text);
                rabbit_log_sites += count_rabbit_log(text);
                if contains_khepri_macro(text) {
                    uses_khepri_macros = true;
                }
                if contains_named_branch(text, "_in_khepri") {
                    saw_khepri_branch = true;
                }
                if contains_named_branch(text, "_in_mnesia") {
                    saw_mnesia_branch = true;
                }
                if let Some(name) = detect_versioned_record_decl(text) {
                    if let Ok(rec) = RecordName::from_str(&name) {
                        introduces_versioned_record.insert(rec);
                    }
                }
            }
        }
        let dominant = match (logger_macro_sites, rabbit_log_sites) {
            (0, 0) => LoggingStyle::None,
            (_, 0) => LoggingStyle::LoggerMacros,
            (0, _) => LoggingStyle::RabbitLogModule,
            _ => LoggingStyle::Mixed,
        };
        if dominant != LoggingStyle::None {
            logging_style.insert(
                path.clone(),
                FileLoggingStyle {
                    dominant,
                    logger_macro_sites,
                    rabbit_log_sites,
                },
            );
        }
    }
    let touches_only_khepri_branch = saw_khepri_branch && !saw_mnesia_branch;
    let touches_dual_branch = saw_khepri_branch && saw_mnesia_branch;
    PatchFacts {
        logging_style,
        khepri_signals: KhepriSignals {
            touches_khepri_module,
            uses_khepri_macros,
            touches_only_khepri_branch,
            touches_dual_branch,
        },
        introduces_versioned_record,
    }
}

fn file_basename_matches_khepri(lower_path: &str) -> bool {
    let basename = lower_path.rsplit('/').next().unwrap_or(lower_path);
    basename.starts_with("rabbit_khepri") || basename.starts_with("khepri_")
}

fn count_logger_macro(text: &str) -> usize {
    let mut n = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i + 5 < bytes.len() {
        if &bytes[i..i + 5] == b"?LOG_" {
            let next = bytes.get(i + 5).copied().unwrap_or(b' ');
            if (next as char).is_ascii_uppercase() {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

fn count_rabbit_log(text: &str) -> usize {
    let mut n = 0usize;
    let needle = b"rabbit_log";
    let bytes = text.as_bytes();
    let nlen = needle.len();
    if bytes.len() < nlen + 1 {
        return 0;
    }
    let mut i = 0usize;
    while i + nlen < bytes.len() {
        if &bytes[i..i + nlen] == needle {
            let prev = if i == 0 { b' ' } else { bytes[i - 1] };
            let next = bytes[i + nlen];
            let prev_alnum = prev.is_ascii_alphanumeric() || prev == b'_';
            let bounded_module = !prev_alnum;
            let followed_by_separator = next == b':' || next == b'_';
            if bounded_module && followed_by_separator {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

fn contains_khepri_macro(text: &str) -> bool {
    let mut i = 0usize;
    let bytes = text.as_bytes();
    // Reading an 8-byte window is valid whenever `i + 8 <= len`.
    while i + 8 <= bytes.len() {
        if &bytes[i..i + 8] == b"?khepri_" {
            return true;
        }
        i += 1;
    }
    false
}

fn contains_named_branch(text: &str, suffix: &str) -> bool {
    let trimmed = text.trim_start();
    let bytes = trimmed.as_bytes();
    let needle = suffix.as_bytes();
    if bytes.len() < needle.len() + 1 {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let next = bytes[i + needle.len()] as char;
            let prev = if i == 0 { ' ' } else { bytes[i - 1] as char };
            let preceded_by_ident = prev.is_ascii_alphanumeric() || prev == '_';
            let bounded_after = !(next.is_ascii_alphanumeric() || next == '_');
            if preceded_by_ident && bounded_after {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// True when every changed `.erl` hunk in `files` is a Variant A
/// unwrap of test-only `-ifdef(TEST)` machinery: dropping the
/// `-ifdef(TEST).` or `-endif.` lines around an `-export` directive,
/// or rewriting `-compile(export_all).` to an explicit `-export`.
/// Variant B unwraps (the function body is also guarded) include real
/// `-spec` and body lines; those return `false` here so the normal
/// analyzer runs.
///
/// Returns `false` if no `.erl` file is touched at all: that case is
/// `NoErlangSurfaceTouched`, which the `TouchedKinds` path already
/// handles.
pub(crate) fn is_only_test_visibility_change(files: &[PatchedFile]) -> bool {
    let mut saw_erlang_file = false;
    for file in files {
        if !matches!(file.language, Language::Erlang) {
            continue;
        }
        if file.binary {
            return false;
        }
        let mut hunk_had_change = false;
        for hunk in &file.hunks {
            for line in &hunk.lines {
                match line {
                    HunkLine::Added(t) | HunkLine::Removed(t) => {
                        if !is_visibility_line(t) {
                            return false;
                        }
                        hunk_had_change = true;
                    }
                    HunkLine::Context(_) => {}
                }
            }
        }
        if hunk_had_change {
            saw_erlang_file = true;
        }
    }
    saw_erlang_file
}

fn is_visibility_line(line: &str) -> bool {
    let stripped = strip_inline_comment(line);
    let trimmed = stripped.trim();
    if trimmed.is_empty() {
        return true;
    }
    let body = trimmed.trim_end_matches('.');
    if matches!(
        body,
        "-ifdef(TEST)" | "-endif" | "-compile(export_all)" | "-compile([export_all])"
    ) {
        return true;
    }
    if trimmed.starts_with("-export(") || trimmed.starts_with("-export_type(") {
        return true;
    }
    looks_like_export_continuation(trimmed)
}

fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

fn looks_like_export_continuation(s: &str) -> bool {
    s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || c == '@'
            || c == '/'
            || c == ','
            || c == '['
            || c == ']'
            || c == '('
            || c == ')'
            || c == '.'
            || c.is_ascii_whitespace()
    })
}

fn detect_versioned_record_decl(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix("-record(")?;
    let comma = rest.find(',')?;
    let name = rest[..comma].trim();
    let bytes = name.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let lower_bytes = lower.as_bytes();
    for i in (0..lower_bytes.len()).rev() {
        if lower_bytes[i] != b'v' {
            continue;
        }
        let mut digits = 0usize;
        let mut p = i + 1;
        while p < lower_bytes.len() && lower_bytes[p].is_ascii_digit() {
            digits += 1;
            p += 1;
        }
        let ended_at_word_boundary = p == lower_bytes.len();
        let preceded_by_underscore = i > 0 && lower_bytes[i - 1] == b'_';
        if digits >= 1 && ended_at_word_boundary && preceded_by_underscore {
            return Some(name.to_owned());
        }
    }
    None
}
