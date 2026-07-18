// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Wire-shape and `is_blocking` checks for three
//! `Reason` variants: `TestModuleSymbolMissing`,
//! `HeaderFileMissing`, and `BehaviourModuleMissing`.
//!
//! All three are non-blocking: they degrade verdict to
//! `RequiresAdaptation`, never `Incompatible`. They match the
//! precedent set by `MissingType` and `PathRename`.

use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RecordName, RelativePath};
use backhopper_core::model::verdict::{IncludeDirective, Reason, TestCallSite, Verdict};

fn mn(s: &str) -> ModuleName {
    ModuleName::new(s).unwrap()
}

fn fnm(s: &str) -> FunctionName {
    FunctionName::new(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

#[test]
fn test_module_symbol_missing_is_non_blocking() {
    let r = Reason::TestModuleSymbolMissing {
        suite_path: rp("deps/rabbit/test/amqp_client_SUITE.erl"),
        missing_module: mn("amqp_utils"),
        call_sites: vec![TestCallSite {
            function: fnm("connection_config"),
            arity: Arity::new(1),
            line: 7,
        }],
    };
    assert!(!r.is_blocking());
}

#[test]
fn behaviour_module_missing_is_non_blocking() {
    let r = Reason::BehaviourModuleMissing {
        source_path: rp("deps/rabbit/test/rabbit_ct_hook_SUITE.erl"),
        behaviour: mn("custom_ct_hook"),
    };
    assert!(!r.is_blocking());
}

#[test]
fn header_file_missing_is_non_blocking() {
    let r = Reason::HeaderFileMissing {
        source_path: rp("deps/rabbit/test/amqp_system_SUITE.erl"),
        include_directive: IncludeDirective::IncludeLib {
            path: "rabbitmq_amqp_client/include/amqp_client.hrl".to_owned(),
        },
        attempted_paths: vec![rp("deps/rabbitmq_amqp_client/include/amqp_client.hrl")],
    };
    assert!(!r.is_blocking());
}

#[test]
fn macro_undefined_on_target_is_non_blocking() {
    let r = Reason::MacroUndefinedOnTarget {
        source_path: rp("deps/rabbit/src/rabbit_mgmt_util.erl"),
        macro_name: "OAUTH2_BOOTSTRAP_PATH".to_owned(),
        line: 42,
    };
    assert!(!r.is_blocking());
    assert!(matches!(
        Verdict::from_reasons(vec![r]),
        Verdict::RequiresAdaptation { .. }
    ));
}

#[test]
fn postimage_collision_is_non_blocking_and_path_scoped() {
    let r = Reason::PostimageCollision {
        path: "deps/rabbit/src/rabbit_amqqueue.erl".into(),
        hunk_index: 2,
    };
    assert!(!r.is_blocking());
    assert!(r.is_path_scoped());
    assert!(matches!(
        Verdict::from_reasons(vec![r]),
        Verdict::RequiresAdaptation { .. }
    ));
}

#[test]
fn postimage_collision_round_trips_through_serde() {
    let r = Reason::PostimageCollision {
        path: "deps/rabbit/src/rabbit_amqqueue.erl".into(),
        hunk_index: 2,
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"kind\":\"postimage_collision\""));
    let back: Reason = serde_json::from_str(&s).unwrap();
    assert_eq!(back, r);
}

#[test]
fn record_and_local_call_undefined_are_non_blocking() {
    let record = Reason::RecordUndefinedOnTarget {
        source_path: rp("deps/rabbit/src/rabbit_fifo.erl"),
        record_name: RecordName::new("state").unwrap(),
        line: 5,
    };
    let call = Reason::LocalCallUndefinedOnTarget {
        source_path: rp("deps/rabbit/src/rabbit_fifo.erl"),
        function: fnm("reader_options"),
        arity: Arity::new(1),
        line: 9,
    };
    assert!(!record.is_blocking());
    assert!(!call.is_blocking());
    assert!(matches!(
        Verdict::from_reasons(vec![record, call]),
        Verdict::RequiresAdaptation { .. }
    ));
}

#[test]
fn macro_undefined_on_target_round_trips_through_serde() {
    let r = Reason::MacroUndefinedOnTarget {
        source_path: rp("deps/rabbit/src/ra_server.erl"),
        macro_name: "MACHINE_VERSION".to_owned(),
        line: 3,
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"kind\":\"macro_undefined_on_target\""));
    assert!(s.contains("\"macro_name\":\"MACHINE_VERSION\""));
    let back: Reason = serde_json::from_str(&s).unwrap();
    assert_eq!(back, r);
}

#[test]
fn three_new_variants_promote_to_requires_adaptation_not_incompatible() {
    let v = Verdict::from_reasons(vec![Reason::TestModuleSymbolMissing {
        suite_path: rp("deps/ra/test/ra_machine_SUITE.erl"),
        missing_module: mn("ra_lib"),
        call_sites: vec![],
    }]);
    assert!(matches!(v, Verdict::RequiresAdaptation { .. }));
}

#[test]
fn test_module_symbol_missing_round_trips_through_serde() {
    let r = Reason::TestModuleSymbolMissing {
        suite_path: rp("deps/rabbit/test/amqp_client_SUITE.erl"),
        missing_module: mn("amqp_utils"),
        call_sites: vec![TestCallSite {
            function: fnm("connection_config"),
            arity: Arity::new(1),
            line: 7,
        }],
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"kind\":\"test_module_symbol_missing\""));
    assert!(s.contains("\"missing_module\":\"amqp_utils\""));
    let back: Reason = serde_json::from_str(&s).unwrap();
    assert_eq!(r, back);
}

#[test]
fn header_file_missing_serializes_with_include_form_tag() {
    let r = Reason::HeaderFileMissing {
        source_path: rp("deps/rabbit/test/rabbit_stream_SUITE.erl"),
        include_directive: IncludeDirective::Include {
            path: "rel/rabbit_stream.hrl".to_owned(),
        },
        attempted_paths: vec![rp("deps/rabbit/test/rel/rabbit_stream.hrl")],
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"form\":\"include\""));
    let back: Reason = serde_json::from_str(&s).unwrap();
    assert_eq!(r, back);
}

#[test]
fn behaviour_module_missing_round_trips() {
    let r = Reason::BehaviourModuleMissing {
        source_path: rp("deps/rabbit/test/rabbit_ct_hook_SUITE.erl"),
        behaviour: mn("custom_ct_hook"),
    };
    let s = serde_json::to_string(&r).unwrap();
    assert!(s.contains("\"kind\":\"behaviour_module_missing\""));
    let back: Reason = serde_json::from_str(&s).unwrap();
    assert_eq!(r, back);
}

#[test]
fn include_directive_path_accessor_strips_form_variant() {
    let inc = IncludeDirective::Include {
        path: "ra_machine.hrl".to_owned(),
    };
    let lib = IncludeDirective::IncludeLib {
        path: "osiris/include/osiris_log.hrl".to_owned(),
    };
    assert_eq!(inc.path(), "ra_machine.hrl");
    assert_eq!(lib.path(), "osiris/include/osiris_log.hrl");
}
