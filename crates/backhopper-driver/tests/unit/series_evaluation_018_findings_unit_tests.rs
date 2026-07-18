// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Driver-side accessors for the `TestModuleSymbolMissing`,
//! `HeaderFileMissing`, and `BehaviourModuleMissing` reasons and the
//! `Diagnostics.missing_test_modules` field. The driver is the
//! consumer surface every downstream tool (q-port, scripts) builds
//! on, so the typed-iterator shape gets a dedicated test file.

use backhopper_driver::CheckPayload;
use serde_json::json;

fn parse(value: serde_json::Value) -> CheckPayload {
    serde_json::from_value(value).expect("CheckPayload deserializes")
}

fn one_pin_with_reasons(reasons: serde_json::Value) -> serde_json::Value {
    json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.0",
            "pins": [ { "project": "ra", "tag": "v2.16.7" } ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "requires_adaptation", "reasons": reasons }
                }
            ],
            "summary": { "compatible": 0, "requires_adaptation": 1, "incompatible": 0 }
        },
        "diagnostics": {
            "missing_test_modules": {
                "deps/rabbit/test/x_SUITE.erl": { "amqp_utils": 3 }
            }
        }
    })
}

#[test]
fn missing_test_modules_diagnostic_round_trips() {
    let eval = parse(one_pin_with_reasons(json!([])));
    let map = eval.missing_test_modules();
    assert_eq!(map.len(), 1);
    let suite = map.keys().next().expect("one suite key");
    assert_eq!(suite.as_str(), "deps/rabbit/test/x_SUITE.erl");
    let helpers = map.get(suite).unwrap();
    assert_eq!(helpers.len(), 1);
    assert_eq!(
        helpers.iter().next().map(|(k, v)| (k.as_str(), *v)),
        Some(("amqp_utils", 3usize))
    );
}

#[test]
fn test_module_symbol_missing_iterator_finds_pinned_reason() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "test_module_symbol_missing",
            "suite_path": "deps/rabbit/test/x_SUITE.erl",
            "missing_module": "amqp_utils",
            "call_sites": [
                { "function": "connection_config", "arity": 1, "line": 42 }
            ]
        }
    ])));
    let findings: Vec<_> = eval.test_module_symbol_missing().collect();
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.pin.pin.project.as_str(), "ra");
    assert_eq!(f.suite_path.as_str(), "deps/rabbit/test/x_SUITE.erl");
    assert_eq!(f.missing_module.as_str(), "amqp_utils");
    assert_eq!(f.call_sites.len(), 1);
    assert_eq!(f.call_sites[0].function.as_str(), "connection_config");
    assert_eq!(f.call_sites[0].line, 42);
}

#[test]
fn header_file_missing_iterator_extracts_include_directive() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "header_file_missing",
            "source_path": "deps/rabbit/src/x.erl",
            "include_directive": {
                "form": "include_lib",
                "path": "rabbitmq_amqp_client/include/amqp_client.hrl"
            },
            "attempted_paths": [
                "deps/rabbitmq_amqp_client/include/amqp_client.hrl",
                "apps/rabbitmq_amqp_client/include/amqp_client.hrl",
                "lib/rabbitmq_amqp_client/include/amqp_client.hrl"
            ]
        }
    ])));
    let findings: Vec<_> = eval.header_file_missing().collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].source_path.as_str(), "deps/rabbit/src/x.erl");
    assert_eq!(
        findings[0].include_directive.path(),
        "rabbitmq_amqp_client/include/amqp_client.hrl"
    );
    assert_eq!(findings[0].attempted_paths.len(), 3);
}

#[test]
fn behaviour_module_missing_iterator_extracts_behaviour_name() {
    let eval = parse(one_pin_with_reasons(json!([
        {
            "kind": "behaviour_module_missing",
            "source_path": "deps/rabbit/test/x_SUITE.erl",
            "behaviour": "custom_ct_hook"
        }
    ])));
    let findings: Vec<_> = eval.behaviour_module_missing().collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].behaviour.as_str(), "custom_ct_hook");
}

#[test]
fn iterators_yield_nothing_when_no_018_reasons_present() {
    let eval = parse(one_pin_with_reasons(json!([])));
    assert_eq!(eval.test_module_symbol_missing().count(), 0);
    assert_eq!(eval.header_file_missing().count(), 0);
    assert_eq!(eval.behaviour_module_missing().count(), 0);
}

#[test]
fn iterators_yield_nothing_on_compatible_pin_even_when_diagnostic_populated() {
    // a Compatible pin carries no reasons, so the three iterators stay
    // empty even when diagnostics exist for another pin or globally
    let value = json!({
        "queried_against": {
            "kind": "series",
            "name": "rabbitmq-4.0",
            "pins": [ { "project": "ra", "tag": "v2.16.7" } ]
        },
        "results": {
            "results": [
                {
                    "pin": { "project": "ra", "tag": "v2.16.7" },
                    "verdict": { "verdict": "compatible" }
                }
            ],
            "summary": { "compatible": 1, "requires_adaptation": 0, "incompatible": 0 }
        },
        "diagnostics": {
            "missing_test_modules": {
                "deps/rabbit/test/x_SUITE.erl": { "amqp_utils": 1 }
            }
        }
    });
    let eval = parse(value);
    assert_eq!(eval.test_module_symbol_missing().count(), 0);
    assert!(!eval.missing_test_modules().is_empty());
}
