// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The indirect-call reason and tally reach a driver consumer: a v13
//! payload with `indirect_call_undefined_on_target` and
//! `indirect_call_checks` deserializes into the typed model, and a
//! pre-change payload without the tally still parses with an empty
//! default.

use backhopper_driver::types::{Diagnostics, IndirectCallForm, IndirectCallTally, Reason};
use serde_json::json;

// Naming the new types through the façade guards the re-exports:
// drop one and this stops compiling.
#[allow(dead_code)]
struct NamesEveryReexport<'a> {
    a: &'a IndirectCallForm,
    b: &'a IndirectCallTally,
}

#[test]
fn the_reason_deserializes_with_its_form() {
    let reason: Reason = serde_json::from_value(json!({
        "kind": "indirect_call_undefined_on_target",
        "source_path": "deps/rabbit/test/maintenance_mode_SUITE.erl",
        "module": "rabbit_queue_type",
        "function": "drain",
        "arity": 1,
        "via": "meck_expect",
        "line": 124
    }))
    .expect("the reason parses");
    let Reason::IndirectCallUndefinedOnTarget { via, arity, .. } = reason else {
        panic!("wrong variant");
    };
    assert_eq!(via, IndirectCallForm::MeckExpect);
    assert_eq!(arity.get(), 1);
    assert_eq!(via.display_form(), "meck:expect");
}

#[test]
fn the_tally_deserializes_inside_diagnostics() {
    let diagnostics: Diagnostics = serde_json::from_value(json!({
        "indirect_call_checks": { "checked": 2, "withheld_dynamic": 1 }
    }))
    .expect("diagnostics parse");
    assert_eq!(diagnostics.indirect_call_checks.checked, 2);
    assert_eq!(diagnostics.indirect_call_checks.withheld_dynamic, 1);
}

#[test]
fn a_payload_without_the_tally_defaults_to_empty() {
    let diagnostics: Diagnostics = serde_json::from_value(json!({})).expect("diagnostics parse");
    assert!(diagnostics.indirect_call_checks.is_empty());
}
