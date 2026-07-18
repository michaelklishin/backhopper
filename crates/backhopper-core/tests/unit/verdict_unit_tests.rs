// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backhopper_core::SymbolRef;
use backhopper_core::model::names::{
    Arity, CommitSha, DependencyName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{
    AlreadyPresent, AlreadyPresentSkipped, BumpStatus, ContentPresence, DepPinDivergence,
    Diagnostics, HunkTally, PinBump, PinVerdict, Reason, SeriesVerdict, TargetMatch,
    TargetMatchKind, Verdict, exit,
};

#[test]
fn no_reasons_means_compatible() {
    assert!(matches!(Verdict::from_reasons(vec![]), Verdict::Compatible));
}

#[test]
fn missing_symbol_is_incompatible() {
    let r = Reason::FileAbsent {
        path: PathBuf::from("ra_log.erl"),
    };
    let v = Verdict::from_reasons(vec![r]);
    assert!(matches!(v, Verdict::Incompatible { .. }));
}

#[test]
fn deprecated_only_is_requires_adaptation() {
    let r = Reason::DeprecatedUsage {
        symbol: SymbolRef::macro_use("MACHINE_VERSION"),
        since: None,
        replacement: None,
    };
    let v = Verdict::from_reasons(vec![r]);
    assert!(matches!(v, Verdict::RequiresAdaptation { .. }));
}

#[test]
fn deprecated_only_does_not_block() {
    let r = Reason::DeprecatedUsage {
        symbol: SymbolRef::macro_use("RA_PROTO_VERSION"),
        since: None,
        replacement: None,
    };
    assert!(!r.is_blocking());
}

#[test]
fn context_drift_only_does_not_block() {
    let r = Reason::ContextDrift {
        path: PathBuf::from("a.erl"),
        hunk_index: 0,
    };
    assert!(!r.is_blocking());
}

#[test]
fn missing_symbol_blocks() {
    let r = Reason::MissingSymbol {
        symbol: SymbolRef::macro_use("OAUTH2_BOOTSTRAP_PATH"),
        first_seen_at_tag: None,
        needs_pin_at_least: None,
        suggested_replacement: None,
    };
    assert!(r.is_blocking());
}

#[test]
fn record_fields_changed_blocks() {
    let r = Reason::RecordFieldsChanged {
        record: RecordName::new("cfg").unwrap(),
        expected: vec![],
        found: vec![],
    };
    assert!(r.is_blocking());
}

#[test]
fn requires_adaptation_only_has_non_blocking_reasons() {
    let r = Reason::DeprecatedUsage {
        symbol: SymbolRef::macro_use("FORCED_MDS_KEY"),
        since: None,
        replacement: None,
    };
    let v = Verdict::from_reasons(vec![r]);
    assert_eq!(v.reasons().len(), 1);
    assert!(!v.is_compatible());
}

#[test]
fn compatible_has_no_reasons_accessor() {
    assert!(Verdict::Compatible.reasons().is_empty());
}

#[test]
fn series_verdict_summarizes_results() {
    let pin1 = Pin::new(
        ProjectName::new("ra").unwrap(),
        TagName::new("v2.8.0").unwrap(),
    );
    let pin2 = Pin::new(
        ProjectName::new("khepri").unwrap(),
        TagName::new("v0.17.0").unwrap(),
    );
    let r1 = PinVerdict::new(pin1, Verdict::Compatible);
    let r2 = PinVerdict::new(
        pin2,
        Verdict::Incompatible {
            reasons: vec![Reason::ArityChanged {
                module: ModuleName::new("ra").unwrap(),
                function: FunctionName::new("process_command").unwrap(),
                expected: Arity::new(2),
                found: vec![Arity::new(3)],
                expected_available_at: None,
                needs_pin_at_least: None,
            }],
        },
    );
    let s = SeriesVerdict::from_results(vec![r1, r2]);
    assert_eq!(s.summary.compatible, 1);
    assert_eq!(s.summary.incompatible, 1);
    assert_eq!(s.summary.requires_adaptation, 0);
    assert_eq!(s.summary.inapplicable, 0);
    assert_eq!(s.worst_exit_code(), exit::NEEDS_ATTENTION);
}

// The is_empty conjunction is hand-maintained: catch a new Diagnostics field left out of the join.
#[test]
fn diagnostics_with_only_already_present_is_not_empty() {
    let mut d = Diagnostics::default();
    assert!(d.is_empty());
    d.already_present = Some(AlreadyPresent {
        identical: None,
        content: None,
    });
    assert!(!d.is_empty());
}

#[test]
fn diagnostics_with_only_a_skip_note_is_not_empty() {
    let d = Diagnostics {
        already_present_skipped: Some(AlreadyPresentSkipped::NoMergeBase),
        ..Default::default()
    };
    assert!(!d.is_empty());
}

#[test]
fn diagnostics_with_only_a_pin_bump_is_not_empty() {
    let d = Diagnostics {
        pin_bumps: vec![PinBump {
            dep: DependencyName::new("cowlib".to_owned()).unwrap(),
            from: Some("hex 2.16.0".to_owned()),
            to: "hex 2.17.1".to_owned(),
            status: None,
        }],
        ..Default::default()
    };
    assert!(!d.is_empty());
}

// Every status state must round-trip so the post-cache fill stays the only writer.
#[test]
fn pin_bump_statuses_round_trip_through_serde() {
    for status in [
        None,
        Some(BumpStatus::Untracked),
        Some(BumpStatus::SnapshotMissing {
            note: "run: backhopper snapshots generate --project cowlib --since 2.17.1".to_owned(),
        }),
        Some(BumpStatus::SnapshotPresent),
    ] {
        let bump = PinBump {
            dep: DependencyName::new("cowlib".to_owned()).unwrap(),
            from: None,
            to: "hex 2.17.1".to_owned(),
            status,
        };
        let json = serde_json::to_string(&bump).unwrap();
        let back: PinBump = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bump);
    }
}

// A serde round-trip failure would silently corrupt cached verdict envelopes.
#[test]
fn new_diagnostics_fields_round_trip_through_serde() {
    let d = Diagnostics {
        already_present: Some(AlreadyPresent {
            identical: Some(TargetMatch {
                commit: CommitSha::new("b".repeat(40)).unwrap(),
                via: TargetMatchKind::PatchId,
                subject: "Backport osiris bloom filter fix".to_owned(),
            }),
            content: Some(ContentPresence {
                pin: ProjectName::new("osiris").unwrap(),
                hunks_already_applied: 2,
                hunks_considered: 3,
                hunks_ambiguous: 1,
                hunks_low_confidence: 0,
                per_file: BTreeMap::from([(
                    "src/osiris_log.erl".parse().unwrap(),
                    HunkTally {
                        applied: 2,
                        considered: 3,
                        ambiguous: 1,
                        low_confidence: 0,
                    },
                )]),
            }),
        }),
        already_present_skipped: Some(AlreadyPresentSkipped::CandidateMissingFromTargetOdb {
            sha: CommitSha::new("c".repeat(40)).unwrap(),
        }),
        dep_pin_divergence: vec![DepPinDivergence {
            dep: DependencyName::new("cowboy").unwrap(),
            source: "hex 2.16.0".to_owned(),
            target: "hex 2.13.0".to_owned(),
        }],
        ..Default::default()
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: Diagnostics = serde_json::from_str(&json).unwrap();
    assert_eq!(back, d);
}

// Envelopes written before v10 lack the new fields: they must deserialize to the empty defaults.
#[test]
fn pre_v10_diagnostics_json_deserializes_with_empty_defaults() {
    let back: Diagnostics = serde_json::from_str("{}").unwrap();
    assert!(back.already_present.is_none());
    assert!(back.already_present_skipped.is_none());
    assert!(back.dep_pin_divergence.is_empty());
}

#[test]
fn diagnostics_with_only_dep_pin_divergence_is_not_empty() {
    let d = Diagnostics {
        dep_pin_divergence: vec![DepPinDivergence {
            dep: DependencyName::new("cowboy").unwrap(),
            source: "hex 2.16.0".to_owned(),
            target: "hex 2.13.0".to_owned(),
        }],
        ..Default::default()
    };
    assert!(!d.is_empty());
}
