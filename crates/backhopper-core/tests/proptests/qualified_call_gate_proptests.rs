// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The qualified-call gate is total and ordered: every module falls in
//! exactly one `ModuleProvenance` arm, and snapshot coverage wins over
//! an in-tree file. Plus the `file_line` translation invariant.

use std::collections::BTreeSet;
use std::str::FromStr;

use backhopper_core::compat::added_lines::file_line;
use backhopper_core::compat::qualified_call_resolve::{ModuleProvenance, classify_module};
use backhopper_core::model::names::{ModuleName, RelativePath};
use proptest::prelude::*;

fn arb_module() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,8}"
}

proptest! {
    #[test]
    fn classify_is_total_and_ordered(
        name in arb_module(),
        covered in any::<bool>(),
        has_path in any::<bool>(),
    ) {
        let module = ModuleName::from_str(&name).unwrap();
        let covered_set: BTreeSet<ModuleName> = if covered {
            BTreeSet::from([module.clone()])
        } else {
            BTreeSet::new()
        };
        let resolve = |_: &ModuleName| -> Option<RelativePath> {
            has_path.then(|| RelativePath::new(format!("src/{name}.erl")).unwrap())
        };
        let provenance = classify_module(&module, &covered_set, &resolve);
        match provenance {
            ModuleProvenance::CoveredBySnapshot => prop_assert!(covered),
            ModuleProvenance::FirstParty { .. } => prop_assert!(!covered && has_path),
            ModuleProvenance::Unknown => prop_assert!(!covered && !has_path),
        }
    }
}

proptest! {
    #[test]
    fn file_line_indexes_or_falls_back(
        map in prop::collection::vec(1u32..100_000, 0..20),
        blob_line in 1u32..40,
    ) {
        let got = file_line(&map, blob_line);
        let idx = (blob_line - 1) as usize;
        if idx < map.len() {
            prop_assert_eq!(got, map[idx]);
        } else {
            prop_assert_eq!(got, blob_line);
        }
    }
}
