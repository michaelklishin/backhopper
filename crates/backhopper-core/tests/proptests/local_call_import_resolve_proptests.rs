// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A call the patch resolves through its own added `-import` is pinned
//! to the presence of that import, not to the function name: with the
//! import and a first-party module that exports the callee it is never
//! a local undefined; without the import and with no target definition
//! it always is.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::local_call_resolve::analyse_local_calls;
use backhopper_core::compat::qualified_call_resolve::{PatchProvided, ReferenceContext};
use backhopper_core::model::names::{ModuleName, RelativePath};
use backhopper_core::model::verdict::Reason;
use proptest::prelude::*;

const CALLER_PATH: &str = "deps/rabbit/test/caller_SUITE.erl";

fn arb_fun() -> impl Strategy<Value = String> {
    "f_[a-z]{1,6}"
}

fn arb_module() -> impl Strategy<Value = String> {
    "m_[a-z]{1,6}"
}

fn call_body(function: &str, arity: usize) -> String {
    let args = vec!["x"; arity].join(", ");
    format!("g() -> {function}({args}).\n")
}

fn module_def(module: &str, function: &str, arity: usize) -> String {
    let params = vec!["_"; arity].join(", ");
    format!("-module({module}).\n-export([{function}/{arity}]).\n{function}({params}) -> ok.\n")
}

fn locally_flagged(reasons: &[Reason], function: &str, arity: usize) -> bool {
    reasons.iter().any(|r| match r {
        Reason::LocalCallUndefinedOnTarget {
            function: f,
            arity: a,
            ..
        } => f.as_str() == function && usize::from(a.get()) == arity,
        _ => false,
    })
}

fn analyse(added: &str, target: &[(&str, &str)]) -> Vec<Reason> {
    let path = RelativePath::new(CALLER_PATH).unwrap();
    let subjects = [AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &[],
    }];
    let target_map: BTreeMap<String, String> = target
        .iter()
        .map(|(p, c)| ((*p).to_owned(), (*c).to_owned()))
        .collect();
    let module_to_path: BTreeMap<ModuleName, RelativePath> = target
        .iter()
        .filter_map(|(p, _)| {
            let stem = p.rsplit('/').next()?.strip_suffix(".erl")?;
            Some((
                ModuleName::from_str(stem).ok()?,
                RelativePath::new(*p).ok()?,
            ))
        })
        .collect();
    let covered: BTreeSet<ModuleName> = BTreeSet::new();
    let patch_added = PatchProvided::default();
    let resolve = |m: &ModuleName| module_to_path.get(m).cloned();
    let read = |p: &RelativePath| target_map.get(p.as_str()).cloned();
    let ctx = ReferenceContext {
        covered_modules: &covered,
        patch_added: &patch_added,
        resolve_module_path: &resolve,
        read_target: &read,
        read_source: None,
    };
    analyse_local_calls(&subjects, &ctx).reasons
}

proptest! {
    // With the import and a first-party module exporting the callee, the imported name is not a local undefined.
    #[test]
    fn a_patch_imported_call_to_an_exporting_module_is_never_local_undefined(
        module in arb_module(),
        function in arb_fun(),
        arity in 0usize..=5,
    ) {
        let added = format!("-import({module}, [{function}/{arity}]).\n{}", call_body(&function, arity));
        let module_path = format!("deps/rabbit/src/{module}.erl");
        let reasons = analyse(
            &added,
            &[
                (CALLER_PATH, "-module(caller_SUITE).\n"),
                (&module_path, &module_def(&module, &function, arity)),
            ],
        );
        prop_assert!(!locally_flagged(&reasons, &function, arity), "reasons: {reasons:?}");
    }

    // Without the import and with no target definition, the call is always a local undefined.
    #[test]
    fn the_same_call_without_the_import_is_always_local_undefined(
        function in arb_fun(),
        arity in 0usize..=5,
    ) {
        let added = call_body(&function, arity);
        let reasons = analyse(&added, &[(CALLER_PATH, "-module(caller_SUITE).\n")]);
        prop_assert!(locally_flagged(&reasons, &function, arity), "reasons: {reasons:?}");
    }
}
