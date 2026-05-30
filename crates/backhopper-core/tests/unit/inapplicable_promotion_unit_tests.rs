// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::names::{Arity, FunctionName, Mfa, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{
    InapplicableReason, PinVerdict, Reason, SeriesVerdict, TouchedKinds, Verdict, exit,
};

fn pin(project: &str, tag: &str) -> Pin {
    Pin::new(
        ProjectName::new(project).unwrap(),
        TagName::new(tag).unwrap(),
    )
}

fn touched_only_docs() -> TouchedKinds {
    TouchedKinds {
        docs: 2,
        ..TouchedKinds::default()
    }
}

fn touched_with_erl() -> TouchedKinds {
    TouchedKinds {
        erl: 1,
        ..TouchedKinds::default()
    }
}

#[test]
fn promote_inapplicable_keeps_compatible_when_erl_was_touched() {
    let pv =
        PinVerdict::new(pin("ra", "v2.0.0"), Verdict::Compatible).with_touched(touched_with_erl());
    let sv = SeriesVerdict::from_results(vec![pv]).promote_inapplicable();
    assert!(matches!(sv.results[0].verdict, Verdict::Compatible));
    assert_eq!(sv.summary.compatible, 1);
    assert_eq!(sv.summary.inapplicable, 0);
}

#[test]
fn promote_inapplicable_flips_zero_tracked_refs_when_only_docs_touched() {
    let pv =
        PinVerdict::new(pin("ra", "v2.0.0"), Verdict::Compatible).with_touched(touched_only_docs());
    let sv = SeriesVerdict::from_results(vec![pv]).promote_inapplicable();
    assert!(matches!(
        sv.results[0].verdict,
        Verdict::Inapplicable {
            reason: InapplicableReason::OnlyDocsTouched
        }
    ));
    assert_eq!(sv.summary.compatible, 0);
    assert_eq!(sv.summary.inapplicable, 1);
}

#[test]
fn promote_inapplicable_does_not_flip_when_tracked_refs_present() {
    let pv = PinVerdict::new(pin("ra", "v2.0.0"), Verdict::Compatible)
        .with_tracked_refs(3)
        .with_touched(touched_only_docs());
    let sv = SeriesVerdict::from_results(vec![pv]).promote_inapplicable();
    assert!(
        matches!(sv.results[0].verdict, Verdict::Compatible),
        "non-zero tracked_refs means a real green signal; do not demote to Inapplicable"
    );
    assert_eq!(sv.summary.compatible, 1);
}

#[test]
fn promote_inapplicable_leaves_incompatible_alone() {
    let pv = PinVerdict::new(
        pin("ra", "v2.0.0"),
        Verdict::Incompatible {
            reasons: vec![Reason::MissingSymbol {
                symbol: SymbolRef::function(Mfa::new(
                    ModuleName::new("m").unwrap(),
                    FunctionName::new("f").unwrap(),
                    Arity::new(1),
                )),
                first_seen_at_tag: None,
                suggested_replacement: None,
            }],
        },
    )
    .with_touched(touched_only_docs());
    let sv = SeriesVerdict::from_results(vec![pv]).promote_inapplicable();
    assert!(matches!(
        sv.results[0].verdict,
        Verdict::Incompatible { .. }
    ));
    assert_eq!(sv.summary.incompatible, 1);
    assert_eq!(sv.summary.inapplicable, 0);
}

#[test]
fn worst_exit_code_severity_ordering() {
    let make = |v: Verdict| PinVerdict::new(pin("p", "v1"), v);
    let incompat = make(Verdict::Incompatible {
        reasons: vec![Reason::FileAbsent {
            path: "x.erl".into(),
        }],
    });
    let requires = make(Verdict::RequiresAdaptation {
        reasons: vec![Reason::ContextDrift {
            path: "x.erl".into(),
            hunk_index: 0,
        }],
    });
    let compat = make(Verdict::Compatible);
    let inapplicable = make(Verdict::Inapplicable {
        reason: InapplicableReason::OnlyDocsTouched,
    });

    let only_incompat = SeriesVerdict::from_results(vec![incompat.clone()]);
    assert_eq!(only_incompat.worst_exit_code(), exit::NEEDS_ATTENTION);

    let incompat_plus_requires =
        SeriesVerdict::from_results(vec![incompat.clone(), requires.clone()]);
    assert_eq!(
        incompat_plus_requires.worst_exit_code(),
        exit::NEEDS_ATTENTION
    );

    let only_requires = SeriesVerdict::from_results(vec![requires.clone()]);
    assert_eq!(only_requires.worst_exit_code(), exit::NEEDS_ATTENTION);

    let compat_plus_inapplicable =
        SeriesVerdict::from_results(vec![compat.clone(), inapplicable.clone()]);
    assert_eq!(compat_plus_inapplicable.worst_exit_code(), exit::OK);

    let only_inapplicable = SeriesVerdict::from_results(vec![inapplicable.clone()]);
    assert_eq!(only_inapplicable.worst_exit_code(), exit::OK);
}
