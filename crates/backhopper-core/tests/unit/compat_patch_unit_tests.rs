// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::compat::patch::{Language, Patch};
use backhopper_core::errors::PatchError;

const SIMPLE_DIFF: &str = "\
diff --git a/src/foo.erl b/src/foo.erl
--- a/src/foo.erl
+++ b/src/foo.erl
@@ -1,3 +1,4 @@
-hello() -> ok.
+hello() -> ok.
+goodbye(X) -> bar:zap(X).
 % unchanged comment
";

#[test]
fn parses_a_minimal_unified_diff() {
    let p = Patch::parse(SIMPLE_DIFF.as_bytes()).unwrap();
    assert_eq!(p.files.len(), 1);
    assert_eq!(p.files[0].language, Language::Erlang);
    assert_eq!(p.files[0].hunks.len(), 1);
}

#[test]
fn analyze_collects_referenced_calls() {
    let p = Patch::parse(SIMPLE_DIFF.as_bytes()).unwrap().analyze();
    let calls: Vec<String> = p
        .referenced()
        .iter()
        .filter_map(|sym| match &sym.kind {
            backhopper_core::SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(calls.iter().any(|c| c == "bar:zap/1"), "got {:?}", calls);
}

#[test]
fn binary_diff_is_marked_binary() {
    let body = "\
diff --git a/x.bin b/x.bin
Binary files a/x.bin and b/x.bin differ
";
    let p = Patch::parse(body.as_bytes()).unwrap();
    assert!(p.files[0].binary);
}

#[test]
fn rejects_oversized_input() {
    let huge = vec![b'a'; 65 * 1024 * 1024];
    let r = Patch::parse(&huge);
    assert!(r.is_err());
}

#[test]
fn rejects_invalid_utf8_with_offset() {
    let mut bytes = b"diff --git a/x b/x\n".to_vec();
    bytes.push(0xFF);
    match Patch::parse(&bytes) {
        Err(PatchError::InvalidUtf8 { offset }) => {
            assert_eq!(
                offset,
                bytes.len() - 1,
                "offset should point at the bad byte"
            );
        }
        other => panic!("expected InvalidUtf8, got {:?}", other),
    }
}
