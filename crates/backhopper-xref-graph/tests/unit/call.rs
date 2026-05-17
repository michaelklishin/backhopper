use backhopper_core::{Arity, FunctionName, Mfa, ModuleName};
use backhopper_xref_graph::{FunctionRef, FunctionSig};

fn mfa(m: &str, f: &str, a: u8) -> Mfa {
    Mfa::new(
        ModuleName::new(m.to_owned()).unwrap(),
        FunctionName::new(f.to_owned()).unwrap(),
        Arity::new(a),
    )
}

#[test]
fn function_sig_display_uses_slash_arity() {
    let sig = FunctionSig::new(
        FunctionName::new("handle_call".to_owned()).unwrap(),
        Arity::new(3),
    );
    assert_eq!(sig.to_string(), "handle_call/3");
}

#[test]
fn concrete_ref_is_not_unresolved() {
    let r = FunctionRef::Concrete(mfa("foo", "bar", 2));
    assert!(!r.is_unresolved());
    assert_eq!(r.as_concrete().unwrap().arity, Arity::new(2));
}

#[test]
fn unresolved_module_keeps_function_and_arity() {
    let r = FunctionRef::UnresolvedModule {
        function: FunctionName::new("handle".to_owned()).unwrap(),
        arity: Some(Arity::new(1)),
    };
    assert!(r.is_unresolved());
    assert!(r.module().is_none());
    assert_eq!(r.arity(), Some(Arity::new(1)));
}

#[test]
fn unresolved_function_keeps_module_and_arity() {
    let r = FunctionRef::UnresolvedFunction {
        module: ModuleName::new("rabbit".to_owned()).unwrap(),
        arity: Some(Arity::new(2)),
    };
    assert!(r.is_unresolved());
    assert_eq!(r.module().unwrap().as_str(), "rabbit");
}

#[test]
fn unresolved_both_carries_only_arity() {
    let r = FunctionRef::UnresolvedBoth { arity: None };
    assert!(r.is_unresolved());
    assert!(r.module().is_none());
    assert_eq!(r.arity(), None);
}

#[test]
fn display_concrete_ref_is_module_function_arity() {
    let r = FunctionRef::Concrete(mfa("foo", "bar", 2));
    assert_eq!(r.to_string(), "foo:bar/2");
}

#[test]
fn display_unresolved_module_uses_placeholder() {
    let r = FunctionRef::UnresolvedModule {
        function: FunctionName::new("f".to_owned()).unwrap(),
        arity: Some(Arity::new(1)),
    };
    assert_eq!(r.to_string(), "'$M_EXPR':f/1");
}

#[test]
fn display_unresolved_function_uses_placeholder() {
    let r = FunctionRef::UnresolvedFunction {
        module: ModuleName::new("m".to_owned()).unwrap(),
        arity: Some(Arity::new(0)),
    };
    assert_eq!(r.to_string(), "m:'$F_EXPR'/0");
}

#[test]
fn display_unresolved_both_uses_both_placeholders_and_negative_arity() {
    let r = FunctionRef::UnresolvedBoth { arity: None };
    assert_eq!(r.to_string(), "'$M_EXPR':'$F_EXPR'/-1");
}

#[test]
fn function_sig_orders_by_name_then_arity() {
    let a0 = FunctionSig::new(FunctionName::new("a".to_owned()).unwrap(), Arity::new(0));
    let a1 = FunctionSig::new(FunctionName::new("a".to_owned()).unwrap(), Arity::new(1));
    let b0 = FunctionSig::new(FunctionName::new("b".to_owned()).unwrap(), Arity::new(0));
    let mut v = [b0.clone(), a1.clone(), a0.clone()];
    v.sort();
    assert_eq!(v, [a0, a1, b0]);
}

#[test]
fn local_function_ref_concrete_carries_function_and_arity() {
    use backhopper_xref_graph::LocalFunctionRef;
    let r = LocalFunctionRef::Concrete {
        function: FunctionName::new("foo".to_owned()).unwrap(),
        arity: Arity::new(2),
    };
    if let LocalFunctionRef::Concrete { function, arity } = r {
        assert_eq!(function.as_str(), "foo");
        assert_eq!(arity.get(), 2);
    } else {
        panic!();
    }
}

#[test]
fn call_kind_round_trips_through_json() {
    use backhopper_xref_graph::CallKind;
    let k = CallKind::Apply;
    let json = serde_json::to_string(&k).unwrap();
    assert_eq!(json, "\"apply\"");
    let back: CallKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, k);
}

#[test]
fn call_target_external_and_local_serialize_distinctly() {
    use backhopper_xref_graph::{CallTarget, LocalFunctionRef};
    let ext = CallTarget::External(FunctionRef::Concrete(mfa("m", "f", 0)));
    let local = CallTarget::Local(LocalFunctionRef::Concrete {
        function: FunctionName::new("g".to_owned()).unwrap(),
        arity: Arity::new(0),
    });
    let ext_json = serde_json::to_string(&ext).unwrap();
    let local_json = serde_json::to_string(&local).unwrap();
    assert!(ext_json.contains("\"target\":\"external\""));
    assert!(local_json.contains("\"target\":\"local\""));
}
