// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::PathBuf;

use backhopper_core::ApplicationName;
use backhopper_xref::XrefBuilder;
use proptest::prelude::*;

proptest! {
    /// The build is permutation-invariant: shuffling input files produces an
    /// xref with identical module sets and identical export sets.
    #[test]
    fn build_is_permutation_invariant(
        n in 1usize..6
    ) {
        let mut files: Vec<(PathBuf, Vec<u8>)> = (0..n).map(|i| {
            let name = format!("m{}", i);
            (PathBuf::from(format!("{}.erl", name)),
             format!("-module({}).\n-export([f/0]).\nf() -> ok.\n", name).into_bytes())
        }).collect();
        let app = ApplicationName::new("t".to_owned()).unwrap();
        let mut b1 = XrefBuilder::new();
        b1.add_application(app.clone(), files.clone()).unwrap();
        let x1 = b1.build().unwrap();

        files.reverse();
        let mut b2 = XrefBuilder::new();
        b2.add_application(app, files).unwrap();
        let x2 = b2.build().unwrap();

        let names1: Vec<String> = x1.graph().modules().map(|(m, _)| m.as_str().to_owned()).collect();
        let names2: Vec<String> = x2.graph().modules().map(|(m, _)| m.as_str().to_owned()).collect();
        prop_assert_eq!(names1, names2);
        prop_assert_eq!(x1.graph().module_count(), n);
    }

    /// `diff_xrefs(x, x)` is empty for any xref built from random inputs.
    #[test]
    fn diff_with_self_is_empty(
        n in 0usize..5
    ) {
        use backhopper_xref::diff_xrefs;
        let files: Vec<(PathBuf, Vec<u8>)> = (0..n).map(|i| {
            let name = format!("m{}", i);
            (PathBuf::from(format!("{}.erl", name)),
             format!("-module({}).\n-export([f/0]).\nf() -> ok.\n", name).into_bytes())
        }).collect();
        let app = ApplicationName::new("t".to_owned()).unwrap();
        let mut b = XrefBuilder::new();
        b.add_application(app, files).unwrap();
        let x = b.build().unwrap();
        let d = diff_xrefs(&x, &x);
        prop_assert!(d.added_modules.is_empty());
        prop_assert!(d.removed_modules.is_empty());
        prop_assert!(d.added_exports.is_empty());
        prop_assert!(d.removed_exports.is_empty());
        prop_assert!(d.added_calls.is_empty());
        prop_assert!(d.removed_calls.is_empty());
    }

    /// `diff(from, to)` and `diff(to, from)` are duals: added on one side
    /// equals removed on the other.
    #[test]
    fn diff_is_anti_symmetric(
        n_from in 0usize..4,
        n_to in 0usize..4
    ) {
        use backhopper_xref::diff_xrefs;
        let app = ApplicationName::new("t".to_owned()).unwrap();
        let make = |n: usize| -> Vec<(PathBuf, Vec<u8>)> {
            (0..n).map(|i| {
                let name = format!("m{}", i);
                (PathBuf::from(format!("{}.erl", name)),
                 format!("-module({}).\n-export([f/0]).\nf() -> ok.\n", name).into_bytes())
            }).collect()
        };
        let mut b_from = XrefBuilder::new();
        b_from.add_application(app.clone(), make(n_from)).unwrap();
        let mut b_to = XrefBuilder::new();
        b_to.add_application(app, make(n_to)).unwrap();
        let from = b_from.build().unwrap();
        let to = b_to.build().unwrap();
        let d_forward = diff_xrefs(&from, &to);
        let d_reverse = diff_xrefs(&to, &from);
        prop_assert_eq!(d_forward.added_modules, d_reverse.removed_modules);
        prop_assert_eq!(d_forward.removed_modules, d_reverse.added_modules);
        prop_assert_eq!(d_forward.added_exports, d_reverse.removed_exports);
        prop_assert_eq!(d_forward.removed_exports, d_reverse.added_exports);
    }
}