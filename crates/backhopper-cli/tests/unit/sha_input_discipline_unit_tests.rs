// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::{Path, PathBuf};

fn commands_dir() -> PathBuf {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_root.join("src").join("commands")
}

fn read_rust_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read commands/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let body = fs::read_to_string(&path).unwrap();
        out.push((path, body));
    }
    out
}

fn nearest_preceding_fn_line(body: &str, call_idx: usize) -> Option<usize> {
    let prefix = &body[..call_idx];
    let mut best: Option<usize> = None;
    for (idx, _) in prefix.match_indices("\nfn ") {
        best = Some(idx + 1);
    }
    for (idx, _) in prefix.match_indices("\npub fn ") {
        if best.map_or(true, |b| idx + 1 > b) {
            best = Some(idx + 1);
        }
    }
    if prefix.starts_with("fn ") || prefix.starts_with("pub fn ") {
        best = Some(0);
    }
    best
}

fn function_has_allow_disallowed_methods(body: &str, call_idx: usize) -> bool {
    let Some(fn_start) = nearest_preceding_fn_line(body, call_idx) else {
        return false;
    };
    let window_start = fn_start.saturating_sub(200);
    let window = &body[window_start..fn_start];
    window.contains("#[allow(clippy::disallowed_methods)]")
}

#[test]
fn no_commit_sha_new_on_operator_input_in_commands() {
    let files = read_rust_files(&commands_dir());
    let mut violations: Vec<String> = Vec::new();
    for (path, body) in &files {
        let mut start = 0;
        while let Some(rel) = body[start..].find("CommitSha::new(") {
            let idx = start + rel;
            if function_has_allow_disallowed_methods(body, idx) {
                start = idx + 14;
                continue;
            }
            let line_no = body[..idx].matches('\n').count() + 1;
            violations.push(format!("{}:{line_no}", path.display()));
            start = idx + 14;
        }
    }
    assert!(
        violations.is_empty(),
        "CommitSha::new is reserved for the resolver layer. \
         Found unguarded call sites:\n  {}\n\
         Operator input must flow through commands::sha_prefix::expand_prefix; \
         internal helpers may keep the call only when marked with \
         #[allow(clippy::disallowed_methods)].",
        violations.join("\n  ")
    );
}

#[test]
fn command_new_git_count_in_check_module_holds_baseline() {
    let body = fs::read_to_string(commands_dir().join("check.rs")).unwrap();
    let git_count = body.matches("Command::new(\"git\"").count();
    let gh_count = body.matches("Command::new(\"gh\"").count();
    assert_eq!(
        git_count, 1,
        "expected exactly one git shell-out in commands/check.rs (the pickaxe walker); \
         a regression here means a new git operation was added without using gix"
    );
    assert_eq!(
        gh_count, 1,
        "expected exactly one gh shell-out in commands/check.rs (gh pr diff); \
         a regression here means a new gh operation was added"
    );
}

#[test]
fn rev_module_does_not_shell_out_to_git_or_gh() {
    let body = fs::read_to_string(commands_dir().join("rev.rs")).unwrap();
    assert!(!body.contains("Command::new(\"git\""));
    assert!(!body.contains("Command::new(\"gh\""));
    assert!(!body.contains("std::process::Command"));
}

#[test]
fn sha_prefix_module_does_not_shell_out() {
    let body = fs::read_to_string(commands_dir().join("sha_prefix.rs")).unwrap();
    assert!(!body.contains("Command::new("));
    assert!(!body.contains("std::process::Command"));
}
