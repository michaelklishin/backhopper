// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;
use std::slice;

use proptest::prelude::*;
use tempfile::TempDir;

use backhopper_core::model::names::ModuleName;
use backhopper_core::suites::SuiteMatcher;
use backhopper_xref_reader::AstSuiteMatcher;

fn write_temp(dir: &TempDir, body: &str) -> PathBuf {
    let p = dir.path().join("x_SUITE.erl");
    std::fs::write(&p, body).unwrap();
    p
}

proptest! {
    #[test]
    fn ast_matcher_panic_free_on_arbitrary_input(text in "[\\x20-\\x7e\\n]{0,512}") {
        let dir = TempDir::new().unwrap();
        let p = write_temp(&dir, &text);
        let mut matcher = AstSuiteMatcher::new();
        let _ = matcher.modules_referenced_in_suite(&p, &[ModuleName::new("rabbit_db").unwrap()]);
    }

    #[test]
    fn explicit_remote_call_always_resolves(
        suite_basename in "[a-z][a-z0-9_]{0,8}_SUITE",
        target_module in "[a-z][a-z0-9_]{0,8}",
        target_fn in "[a-z][a-z0-9_]{0,8}",
    ) {
        prop_assume!(target_module != "ok" && target_fn != "ok");
        let body = format!(
            "-module({suite_basename}).\n\
             -export([go/0]).\n\
             go() -> {target_module}:{target_fn}(1).\n"
        );
        let dir = TempDir::new().unwrap();
        let p = write_temp(&dir, &body);
        let mut matcher = AstSuiteMatcher::new();
        let module = ModuleName::new(&target_module).unwrap();
        let refs = matcher.modules_referenced_in_suite(&p, slice::from_ref(&module));
        prop_assert!(refs.contains(&module));
        prop_assert!(matcher.parsed_cleanly(&p));
    }
}
