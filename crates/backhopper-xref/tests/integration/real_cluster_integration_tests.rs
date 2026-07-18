// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end analysis over complete, unmodified aten modules kept
//! verbatim under `tests/fixtures`. Asserts the essential outcomes only:
//! the real internal edge resolves and the calls that leave the analyzed
//! set are reported.

use std::path::PathBuf;

use backhopper_core::{ApplicationName, ModuleName};
use backhopper_xref::{AnalysisResult, XrefBuilder};

const ATEN: &str = include_str!("../fixtures/aten.erl");
const ATEN_DETECTOR: &str = include_str!("../fixtures/aten_detector.erl");

fn mname(s: &str) -> ModuleName {
    ModuleName::new(s.to_owned()).unwrap()
}

#[test]
fn aten_cluster_resolves_internal_edge_and_reports_otp_calls() {
    let mut b = XrefBuilder::new();
    b.add_application(
        ApplicationName::new("aten".to_owned()).unwrap(),
        vec![
            (PathBuf::from("aten.erl"), ATEN.as_bytes().to_vec()),
            (
                PathBuf::from("aten_detector.erl"),
                ATEN_DETECTOR.as_bytes().to_vec(),
            ),
        ],
    )
    .unwrap();
    let x = b.build().unwrap();

    assert_eq!(x.graph().modules().count(), 2);

    // aten:register/1 calls aten_detector:register/1, a real internal edge.
    let deps = x.module_call(&mname("aten"));
    assert!(deps.entries.contains(&mname("aten_detector")));

    // That internal call resolves, while calls leaving the two-module set
    // (OTP's application, gen_server) are reported as undefined.
    let undefined = x.undefined_function_calls();
    assert!(!undefined.is_clean());
    let callees: Vec<String> = undefined
        .entries
        .iter()
        .map(|e| e.callee.to_string())
        .collect();
    assert!(
        !callees.iter().any(|c| c.contains("aten_detector:register")),
        "internal edge should resolve: {callees:?}"
    );
}
