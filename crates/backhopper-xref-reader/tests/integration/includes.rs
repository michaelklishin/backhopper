use std::path::PathBuf;

use backhopper_xref_graph::{CallTarget, FunctionRef};
use backhopper_xref_reader::{ReadWarning, SourceReader};

fn external_names(modules: &[backhopper_xref_reader::ModuleData]) -> Vec<String> {
    modules
        .iter()
        .flat_map(|m| m.external_calls.iter())
        .filter_map(|c| match &c.callee {
            CallTarget::External(FunctionRef::Concrete(mfa)) => {
                Some(format!("{}:{}/{}", mfa.module, mfa.function, mfa.arity))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn macros_from_relative_include_are_available_in_the_consumer() {
    let files = vec![
        (
            PathBuf::from("deps/rabbit/src/m.erl"),
            b"-module(m).\n\
              -include(\"shared.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?SERVER:start(1).\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/shared.hrl"),
            b"-define(SERVER, my_module).\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(names.iter().any(|n| n == "my_module:start/1"), "{names:?}");
}

#[test]
fn macros_from_include_lib_resolve_through_app_root() {
    let files = vec![
        (
            PathBuf::from("deps/consumer/src/m.erl"),
            b"-module(m).\n\
              -include_lib(\"shared/include/api.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?LOG(debug, \"hi\").\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/shared/include/api.hrl"),
            b"-define(LOG(L, M), logger:log(L, M)).\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(names.iter().any(|n| n == "logger:log/2"), "{names:?}");
}

#[test]
fn missing_include_emits_warning_but_does_not_panic() {
    let files = vec![(
        PathBuf::from("deps/rabbit/src/m.erl"),
        b"-module(m).\n\
          -include(\"nonexistent.hrl\").\n\
          -export([go/0]).\n\
          go() -> ok.\n"
            .to_vec(),
    )];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    assert!(
        out.warnings.iter().any(|w| matches!(
            w,
            ReadWarning::UnresolvedInclude { target, .. }
                if target.to_string_lossy() == "nonexistent.hrl"
        )),
        "warnings={:?}",
        out.warnings
    );
}

#[test]
fn nested_include_from_a_header_pulls_in_transitive_macros() {
    let files = vec![
        (
            PathBuf::from("deps/rabbit/src/m.erl"),
            b"-module(m).\n\
              -include(\"a.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?SERVER:start(1).\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/a.hrl"),
            b"-include(\"b.hrl\").\n".to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/b.hrl"),
            b"-define(SERVER, my_module).\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(
        names.iter().any(|n| n == "my_module:start/1"),
        "transitive include did not resolve: {names:?}"
    );
}

#[test]
fn deeply_nested_includes_resolve_to_arbitrary_depth() {
    let files = vec![
        (
            PathBuf::from("deps/rabbit/src/m.erl"),
            b"-module(m).\n\
              -include(\"a.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?SERVER:start(1).\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/a.hrl"),
            b"-include(\"b.hrl\").\n".to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/b.hrl"),
            b"-include(\"c.hrl\").\n".to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/c.hrl"),
            b"-include(\"d.hrl\").\n".to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/d.hrl"),
            b"-define(SERVER, deep_module).\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(
        names.iter().any(|n| n == "deep_module:start/1"),
        "4-level include chain: {names:?}"
    );
}

#[test]
fn include_cycle_does_not_infinite_loop() {
    let files = vec![
        (
            PathBuf::from("deps/rabbit/src/m.erl"),
            b"-module(m).\n\
              -include(\"a.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?A:f().\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/a.hrl"),
            b"-include(\"b.hrl\").\n\
              -define(A, alpha).\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/src/b.hrl"),
            b"-include(\"a.hrl\").\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(
        names.iter().any(|n| n == "alpha:f/0"),
        "cycle still resolves macros from first visit: {names:?}"
    );
}

#[test]
fn include_basename_falls_back_when_path_is_relative() {
    let files = vec![
        (
            PathBuf::from("deps/rabbit/src/m.erl"),
            b"-module(m).\n\
              -include(\"../include/shared.hrl\").\n\
              -export([go/0]).\n\
              go() -> ?SERVER:start(1).\n"
                .to_vec(),
        ),
        (
            PathBuf::from("deps/rabbit/include/shared.hrl"),
            b"-define(SERVER, my_module).\n".to_vec(),
        ),
    ];
    let reader = SourceReader::new();
    let out = reader.read_tree(files).expect("read_tree ok");
    let names = external_names(&out.modules);
    assert!(names.iter().any(|n| n == "my_module:start/1"), "{names:?}");
}
