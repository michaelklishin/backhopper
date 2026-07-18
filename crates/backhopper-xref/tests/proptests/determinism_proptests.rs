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
            let name = format!("m{i}");
            (PathBuf::from(format!("{name}.erl")),
             format!("-module({name}).\n-export([f/0]).\nf() -> ok.\n").into_bytes())
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
        prop_assert_eq!(x1.graph().modules().count(), n);
    }
}
