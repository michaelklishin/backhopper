use std::path::PathBuf;

use backhopper_core::SymbolRef;
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::verdict::{PinVerdict, Reason, SeriesVerdict, Verdict};

#[test]
fn no_reasons_means_compatible() {
    assert!(matches!(Verdict::from_reasons(vec![]), Verdict::Compatible));
}

#[test]
fn missing_symbol_is_incompatible() {
    let r = Reason::FileAbsent {
        path: PathBuf::from("foo.erl"),
    };
    let v = Verdict::from_reasons(vec![r]);
    assert!(matches!(v, Verdict::Incompatible { .. }));
}

#[test]
fn deprecated_only_is_requires_adaptation() {
    let r = Reason::DeprecatedUsage {
        symbol: SymbolRef::macro_use("X"),
        since: None,
        replacement: None,
    };
    let v = Verdict::from_reasons(vec![r]);
    assert!(matches!(v, Verdict::RequiresAdaptation { .. }));
}

#[test]
fn series_verdict_summarizes_results() {
    let pin1 = Pin::new(ProjectName::new("a").unwrap(), TagName::new("v1").unwrap());
    let pin2 = Pin::new(ProjectName::new("b").unwrap(), TagName::new("v1").unwrap());
    let r1 = PinVerdict {
        pin: pin1,
        verdict: Verdict::Compatible,
    };
    let r2 = PinVerdict {
        pin: pin2,
        verdict: Verdict::Incompatible {
            reasons: vec![Reason::ArityChanged {
                module: ModuleName::new("foo").unwrap(),
                function: FunctionName::new("bar").unwrap(),
                expected: Arity::new(2),
                found: vec![Arity::new(3)],
            }],
        },
    };
    let s = SeriesVerdict::from_results(vec![r1, r2]);
    assert_eq!(s.summary.compatible, 1);
    assert_eq!(s.summary.incompatible, 1);
    assert_eq!(s.summary.requires_adaptation, 0);
    assert_eq!(s.worst_exit_code(), 1);
}
