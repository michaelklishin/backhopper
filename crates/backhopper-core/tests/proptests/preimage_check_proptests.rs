// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use proptest::collection::vec;
use proptest::proptest;

use backhopper_core::compat::patch::{EvaluationFiles, Patch};
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_test_support::{canonical_snapshot, snapshot_header};

fn snapshot() -> Snapshot<state::Canonical> {
    canonical_snapshot(snapshot_header("demo", "v1.0.0"), vec![])
}

fn pin() -> Pin {
    Pin::new(
        ProjectName::new("demo").unwrap(),
        TagName::new("v1.0.0").unwrap(),
    )
}

proptest! {
    #[test]
    fn classify_preimage_does_not_panic_on_arbitrary_target(
        target_lines in vec("[\\x20-\\x7e]{0,32}", 0..30),
    ) {
        let target = format!("{}\n", target_lines.join("\n"));
        let patch = "\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -1,3 +1,4 @@
 -module(x).
 -export([f/1]).
+helper() -> ok.
 f(X) -> X.
";
        let files = EvaluationFiles::new()
            .with(PathBuf::from("src/x.erl"), Some(target.into_bytes()));
        let _ = Patch::parse(patch.as_bytes())
            .unwrap()
            .analyze()
            .against_series_with_files(&[(pin(), snapshot(), files)]);
    }

    #[test]
    fn classify_preimage_does_not_panic_on_random_hunk_starts(
        old_start in 1usize..=100,
        new_start in 1usize..=100,
        target_lines in vec("[\\x20-\\x7e]{0,32}", 0..30),
    ) {
        let target = target_lines.join("\n");
        let patch = format!("\
diff --git a/src/x.erl b/src/x.erl
--- a/src/x.erl
+++ b/src/x.erl
@@ -{old_start},3 +{new_start},4 @@
 context_a
 context_b
+added_line
 context_c
");
        let files = EvaluationFiles::new()
            .with(PathBuf::from("src/x.erl"), Some(target.into_bytes()));
        if let Ok(p) = Patch::parse(patch.as_bytes()) {
            let _ = p
                .analyze()
                .against_series_with_files(&[(pin(), snapshot(), files)]);
        }
    }
}
