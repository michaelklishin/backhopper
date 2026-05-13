use backhopper_erlang::attributes::{ParsedAttribute, classify};
use backhopper_erlang::tokenizer::iterate_attributes;

fn classify_first(src: &str) -> ParsedAttribute {
    let blocks = iterate_attributes(src);
    classify(&blocks[0]).expect("classified")
}

#[test]
fn classifies_module_attribute() {
    let attr = classify_first("-module(ra).\n");
    match attr {
        ParsedAttribute::Module(name) => assert_eq!(name.as_str(), "ra"),
        other => panic!("got {:?}", other),
    }
}

#[test]
fn classifies_export_with_multiple_entries() {
    let attr = classify_first("-export([init/1, apply/3]).\n");
    match attr {
        ParsedAttribute::Export(list) => {
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].name.as_str(), "init");
            assert_eq!(list[0].arity.get(), 1);
            assert_eq!(list[1].name.as_str(), "apply");
            assert_eq!(list[1].arity.get(), 3);
        }
        other => panic!("got {:?}", other),
    }
}

#[test]
fn classifies_export_type() {
    let attr = classify_first("-export_type([range/0]).\n");
    match attr {
        ParsedAttribute::ExportType(list) => {
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].name, "range");
            assert_eq!(list[0].arity, 0);
        }
        other => panic!("got {:?}", other),
    }
}

#[test]
fn classifies_behaviour_and_behavior_spellings() {
    let a = classify_first("-behaviour(gen_server).\n");
    let b = classify_first("-behavior(gen_server).\n");
    assert!(matches!(a, ParsedAttribute::Behaviour(_)));
    assert!(matches!(b, ParsedAttribute::Behaviour(_)));
}

#[test]
fn classifies_callback_and_spec() {
    let cb = classify_first("-callback init(Conf :: term()) -> state().\n");
    assert!(matches!(cb, ParsedAttribute::Callback(_)));
    let sp = classify_first("-spec foo(X :: integer()) -> ok | {error, term()}.\n");
    assert!(matches!(sp, ParsedAttribute::Spec(_)));
}

#[test]
fn classifies_optional_callbacks_and_on_load() {
    let oc = classify_first("-optional_callbacks([overview/1]).\n");
    assert!(matches!(oc, ParsedAttribute::OptionalCallbacks(_)));
    let ol = classify_first("-on_load(init/0).\n");
    assert!(matches!(ol, ParsedAttribute::OnLoad(_)));
}

#[test]
fn classifies_doc_hidden() {
    let d = classify_first("-doc(hidden).\n");
    assert!(matches!(d, ParsedAttribute::DocHidden));
}
