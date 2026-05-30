// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use proptest::prelude::*;
use time::OffsetDateTime;

use backhopper_core::compat::patch::{EvaluationContext, Patch};
use backhopper_core::compat::scope::PinScope;
use backhopper_core::model::names::{
    Arity, CommitSha, FunctionName, ModuleName, ProjectName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{
    FunArity, Module, Snapshot, SnapshotHeader, Visibility, state,
};

fn arb_lower_atom() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,7}".prop_map(|s| s)
}

fn header(project: &str) -> SnapshotHeader {
    SnapshotHeader {
        project: ProjectName::new(project).unwrap(),
        tag: TagName::new("v1.0.0").unwrap(),
        branch: None,
        commit: CommitSha::new("0".repeat(40)).unwrap(),
        scanned_paths: vec!["src/**/*.erl".into()],
        apps_scanned: Vec::new(),
        generated_by: "proptest".into(),
        generated_at: OffsetDateTime::from_unix_timestamp(0).unwrap(),
        extractor_version: String::new(),
    }
}

fn module_with_export(name: &str, fun: &str, arity: u8) -> Module {
    let mut m = Module::new(ModuleName::new(name).unwrap());
    m.visibility = Visibility::Public;
    m.exports.push(FunArity {
        name: FunctionName::new(fun).unwrap(),
        arity: Arity::new(arity),
    });
    m
}

proptest! {
    #[test]
    fn tracked_refs_count_equals_unique_in_scope_module_calls(
        tracked_modules in prop::collection::hash_set(arb_lower_atom(), 1..5),
        untracked_modules in prop::collection::hash_set(arb_lower_atom(), 0..5),
        repeats in 1u8..=4
    ) {
        let mut tracked: Vec<String> = tracked_modules.iter().cloned().collect();
        tracked.sort();
        let mut untracked: Vec<String> = untracked_modules
            .iter()
            .filter(|m| !tracked_modules.contains(*m))
            .cloned()
            .collect();
        untracked.sort();
        let project = "demo";
        let modules: Vec<Module> = tracked
            .iter()
            .map(|m| module_with_export(m, "noop", 0))
            .collect();
        let snap: Snapshot<state::Canonical> =
            Snapshot::from_extracted(header(project), modules, vec![]).into_canonical();
        let scope = PinScope::from_snapshot(ProjectName::new(project).unwrap(), &snap, []);
        let context = EvaluationContext::new(
            Pin::new(
                ProjectName::new(project).unwrap(),
                TagName::new("v1.0.0").unwrap(),
            ),
            snap,
            scope,
        );
        let mut diff = String::from(
            "diff --git a/x.erl b/x.erl\n--- a/x.erl\n+++ b/x.erl\n@@ -1,1 +1,99 @@\n -module(x).\n",
        );
        for m in tracked.iter().chain(untracked.iter()) {
            for _ in 0..repeats {
                diff.push_str(&format!("+go() -> {m}:noop().\n"));
            }
        }
        let eval = Patch::parse(diff.as_bytes())
            .unwrap()
            .analyze()
            .evaluate_series(&[context]);
        let r0 = &eval.verdict.results[0];
        prop_assert_eq!(r0.tracked_refs, tracked.len());
        for m in &untracked {
            let count = eval
                .diagnostics
                .untracked_calls
                .get(&ModuleName::new(m).unwrap())
                .copied();
            prop_assert_eq!(
                count,
                Some(1),
                "module {} should appear exactly once after dedup",
                m
            );
        }
    }
}
