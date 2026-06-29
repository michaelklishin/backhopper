// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Output shape of the shared JSONL emitter.

use serde::Serialize;

use backhopper_cli::output::emit_jsonl;

#[derive(Serialize)]
struct Candidate {
    sha: &'static str,
    verdict: &'static str,
}

#[test]
fn emit_jsonl_writes_one_object_per_line_with_trailing_newline() {
    let rows = vec![
        Candidate {
            sha: "abc1234",
            verdict: "Compatible",
        },
        Candidate {
            sha: "def5678",
            verdict: "Incompatible",
        },
    ];
    let mut buf: Vec<u8> = Vec::new();
    emit_jsonl(&mut buf, &rows).expect("emit ok");
    let text = String::from_utf8(buf).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], r#"{"sha":"abc1234","verdict":"Compatible"}"#);
    assert_eq!(lines[1], r#"{"sha":"def5678","verdict":"Incompatible"}"#);
    assert!(text.ends_with('\n'));
}

#[test]
fn emit_jsonl_on_empty_slice_writes_nothing() {
    let rows: Vec<Candidate> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    emit_jsonl(&mut buf, &rows).expect("emit ok");
    assert!(buf.is_empty());
}
