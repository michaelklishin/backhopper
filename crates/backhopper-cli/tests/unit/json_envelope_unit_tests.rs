// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Drift-guard: known payloads serialized through `serde_json` must
//! match the checked-in fixtures under `tests/fixtures/json/`. If a
//! Rust type changes shape, this test fails until the fixture is
//! intentionally regenerated.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::names::{
    Arity, DependencyName, FunctionName, Mfa, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    BumpStatus, Diagnostics, NeedsPin, PinBump, PinVerdict, Reason, Unanalyzed, Verdict,
};
use std::str::FromStr;

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("json");
    p.push(name);
    p
}

fn assert_matches_fixture<T: Serialize>(value: &T, fixture: &str) {
    let path = fixture_path(fixture);
    let expected_raw =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let expected: serde_json::Value = serde_json::from_str(&expected_raw)
        .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
    let actual = serde_json::to_value(value).expect("serialize");
    assert_eq!(
        actual,
        expected,
        "fixture drift in {}: regenerate after intentional schema changes",
        path.display()
    );
}

#[test]
fn pin_verdict_compatible_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        ),
        Verdict::Compatible,
    )
    .with_tracked_refs(3);
    assert_matches_fixture(&pv, "verdict_compatible.json");
}

#[test]
fn pin_verdict_with_reasons_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        ),
        Verdict::Incompatible {
            reasons: vec![
                Reason::ArityChanged {
                    module: ModuleName::new("ra").unwrap(),
                    function: FunctionName::new("start").unwrap(),
                    expected: Arity::new(2),
                    found: vec![Arity::new(0)],
                    expected_available_at: None,
                    needs_pin_at_least: None,
                },
                Reason::NowHidden {
                    module: ModuleName::new("ra_internal").unwrap(),
                },
            ],
        },
    )
    .with_tracked_refs(2);
    assert_matches_fixture(&pv, "verdict_with_reasons.json");
}

#[test]
fn diagnostics_populated_matches_fixture() {
    let mut diag = Diagnostics::default();
    diag.untracked_calls
        .insert(ModuleName::new("lists").unwrap(), 5);
    diag.untracked_calls
        .insert(ModuleName::new("io").unwrap(), 2);
    diag.untracked_records
        .insert(RecordName::new("config").unwrap(), 1);
    diag.unanalyzed = Unanalyzed {
        apply: 3,
        variable_dispatch: 1,
    };
    assert_matches_fixture(&diag, "diagnostics_populated.json");
}

#[test]
fn empty_diagnostics_serializes_to_empty_object() {
    let diag = Diagnostics::default();
    let actual = serde_json::to_value(&diag).unwrap();
    assert_eq!(actual, serde_json::json!({}));
}

#[test]
fn pin_verdict_with_clause_mismatch_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        ),
        Verdict::Incompatible {
            reasons: vec![Reason::ClauseMismatch {
                module: ModuleName::new("ra").unwrap(),
                function: FunctionName::new("mode").unwrap(),
                arity: Arity::new(1),
                call_args: vec![ArgShape::Atom {
                    name: "restart".into(),
                }],
                pin_clauses: vec![
                    vec![ArgShape::Atom {
                        name: "start".into(),
                    }],
                    vec![ArgShape::Atom {
                        name: "stop".into(),
                    }],
                ],
            }],
        },
    )
    .with_tracked_refs(1);
    assert_matches_fixture(&pv, "verdict_with_clause_mismatch.json");
}

#[test]
fn pin_verdict_with_untracked_module_missing_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        ),
        Verdict::Incompatible {
            reasons: vec![Reason::UntrackedModuleMissing {
                module: ModuleName::new("rabbit_mgmt_wm_user").unwrap(),
            }],
        },
    );
    assert_matches_fixture(&pv, "verdict_with_untracked_module_missing.json");
}

#[test]
fn pin_verdict_with_unsupported_file_type_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("ra").unwrap(),
            TagName::new("v2.0.0").unwrap(),
        ),
        Verdict::RequiresAdaptation {
            reasons: vec![Reason::UnsupportedFileType {
                path: PathBuf::from("lib/rabbitmq/cli/ctl/commands/status_command.ex"),
            }],
        },
    );
    assert_matches_fixture(&pv, "verdict_with_unsupported_file_type.json");
}

// locks the bump-first wire shape: first_seen_at_tag plus needs_pin_at_least on a requires_adaptation reason
#[test]
fn pin_verdict_with_bump_first_reason_matches_fixture() {
    let pv = PinVerdict::new(
        Pin::new(
            ProjectName::new("cowlib").unwrap(),
            TagName::new("2.13.0").unwrap(),
        ),
        Verdict::RequiresAdaptation {
            reasons: vec![Reason::MissingSymbol {
                symbol: SymbolRef::function(Mfa::from_str("cow_http:ensure_token/1").unwrap()),
                first_seen_at_tag: Some(TagName::new("2.17.0").unwrap()),
                needs_pin_at_least: Some(NeedsPin {
                    project: ProjectName::new("cowlib").unwrap(),
                    tag: TagName::new("2.17.0").unwrap(),
                }),
                suggested_replacement: None,
            }],
        },
    )
    .with_tracked_refs(1);
    assert_matches_fixture(&pv, "verdict_with_bump_first_reason.json");
}

// pins the wire shape of every BumpStatus state plus the cached and introduced-pin forms
#[test]
fn diagnostics_with_pin_bumps_matches_fixture() {
    let bump = |dep: &str, from: Option<&str>, to: &str, status: Option<BumpStatus>| PinBump {
        dep: DependencyName::new(dep.to_owned()).unwrap(),
        from: from.map(str::to_owned),
        to: to.to_owned(),
        status,
    };
    let diagnostics = Diagnostics {
        pin_bumps: vec![
            bump(
                "cowlib",
                Some("hex 2.16.0"),
                "hex 2.17.1",
                Some(BumpStatus::SnapshotMissing {
                    note: "run: backhopper snapshots generate --project cowlib --since 2.17.1"
                        .into(),
                }),
            ),
            bump("emqtt", None, "git v1.14.0", Some(BumpStatus::Untracked)),
            bump(
                "ra",
                Some("hex 3.1.5"),
                "hex 3.1.6",
                Some(BumpStatus::SnapshotPresent),
            ),
            bump("osiris", Some("hex 1.9.0"), "hex 1.9.1", None),
        ],
        ..Diagnostics::default()
    };
    assert_matches_fixture(&diagnostics, "diagnostics_with_pin_bumps.json");
}
