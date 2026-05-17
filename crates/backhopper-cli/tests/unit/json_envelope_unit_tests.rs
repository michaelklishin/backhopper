//! Drift-guard: known payloads serialized through `serde_json` must
//! match the checked-in fixtures under `tests/fixtures/json/`. If a
//! Rust type changes shape, this test fails until the fixture is
//! intentionally regenerated.

use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::model::names::{
    Arity, FunctionName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{Diagnostics, PinVerdict, Reason, Unanalyzed, Verdict};

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
                path: PathBuf::from("lib/foo.ex"),
            }],
        },
    );
    assert_matches_fixture(&pv, "verdict_with_unsupported_file_type.json");
}
