// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end extraction over complete, unmodified RabbitMQ-ecosystem
//! modules kept verbatim under `tests/fixtures` (with their own license
//! headers). Each test asserts only the essential surface, so a real
//! parsing regression shows up without pinning every detail.

use backhopper_erlang::ErlangExtractor;

const ATEN: &str = include_str!("../fixtures/aten.erl");
const ATEN_DETECTOR: &str = include_str!("../fixtures/aten_detector.erl");
const RA_COUNTERS: &str = include_str!("../fixtures/ra_counters.erl");
const RA_MACHINE: &str = include_str!("../fixtures/ra_machine.erl");
const SESHAT_COUNTERS_SERVER: &str = include_str!("../fixtures/seshat_counters_server.erl");

// aten: three exports and two specs over guarded clauses that delegate to aten_detector
#[test]
fn extracts_aten_surface() {
    let m = ErlangExtractor::default().extract_module(ATEN).unwrap();
    assert_eq!(m.name.as_str(), "aten");
    assert_eq!(m.exports.len(), 3);
    assert_eq!(m.specs.len(), 2);
}

// ra_machine: large callback set, long -optional_callbacks list, exported
// helpers, and -type declarations with nested map and union right-hand sides
#[test]
fn extracts_ra_machine_behaviour_surface() {
    let m = ErlangExtractor::default()
        .extract_module(RA_MACHINE)
        .unwrap();
    assert_eq!(m.name.as_str(), "ra_machine");
    assert_eq!(m.callbacks.len(), 13);
    assert!(!m.optional_callbacks.is_empty());
    assert!(m.callbacks.iter().any(|c| c.name.as_str() == "init"));
    assert!(m.types.len() >= 5);
}

// aten_detector: a gen_server with two -define macros, a -record, and a
// -ifdef(TEST) block whose functions stay out of the public export set
#[test]
fn extracts_aten_detector_surface() {
    let m = ErlangExtractor::default()
        .extract_module(ATEN_DETECTOR)
        .unwrap();
    assert_eq!(m.name.as_str(), "aten_detector");
    assert_eq!(m.behaviours.len(), 1);
    assert_eq!(m.exports.len(), 9);
}

// ra_counters: multi-line -spec set with a nested-map return type, an -include, and a -type
#[test]
fn extracts_ra_counters_surface() {
    let m = ErlangExtractor::default()
        .extract_module(RA_COUNTERS)
        .unwrap();
    assert_eq!(m.name.as_str(), "ra_counters");
    assert_eq!(m.exports.len(), 8);
    assert!(m.specs.len() >= 7, "specs={}", m.specs.len());
}

// seshat_counters_server: a behaviour, an -include, a -record, a -define, and the full gen_server callback surface
#[test]
fn extracts_seshat_counters_server_surface() {
    let m = ErlangExtractor::default()
        .extract_module(SESHAT_COUNTERS_SERVER)
        .unwrap();
    assert_eq!(m.name.as_str(), "seshat_counters_server");
    assert_eq!(m.behaviours.len(), 1);
    // start_link/0, the six gen_server callbacks, and four table ops
    assert_eq!(m.exports.len(), 11);
}
