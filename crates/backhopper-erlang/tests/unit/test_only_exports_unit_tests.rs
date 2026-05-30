// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::model::snapshot::{IfdefGuardKind, TestExportVariant};
use backhopper_erlang::ErlangExtractor;

fn extract(source: &str) -> backhopper_core::model::snapshot::Module {
    ErlangExtractor::default()
        .extract_module(source)
        .expect("module")
}

#[test]
fn test_only_export_is_recorded_with_export_line() {
    let src = r#"
-module(rabbit_fifo).

-export([init/1]).

-ifdef(TEST).
-export([update_header/4, chunk_disk_msgs/3]).
-endif.

init(_) -> ok.
"#;
    let m = extract(src);
    assert_eq!(
        m.test_only_exports.len(),
        2,
        "got: {:?}",
        m.test_only_exports
    );
    let names: Vec<_> = m
        .test_only_exports
        .iter()
        .map(|t| (t.function.as_str().to_owned(), t.arity.get(), t.variant))
        .collect();
    assert!(
        names.contains(&("update_header".to_owned(), 4, TestExportVariant::A)),
        "got: {names:?}"
    );
    assert!(
        names.contains(&("chunk_disk_msgs".to_owned(), 3, TestExportVariant::A)),
        "got: {names:?}"
    );
    for entry in &m.test_only_exports {
        assert!(
            entry.export_line >= 1,
            "export_line should be 1-based, got {entry:?}"
        );
    }
}

#[test]
fn non_test_export_is_not_recorded_as_test_only() {
    let src = r#"
-module(x).
-export([foo/0]).
foo() -> ok.
"#;
    let m = extract(src);
    assert!(
        m.test_only_exports.is_empty(),
        "got: {:?}",
        m.test_only_exports
    );
}

#[test]
fn ifdef_define_inside_test_block_is_recorded() {
    let src = r#"
-module(x).

-ifdef(TEST).
-define(FORCED_MDS_KEY, {?MODULE, forced_mds}).
-define(PT_OVERRIDDEN_NODES, {?MODULE, overridden_nodes}).
-endif.

foo() -> ok.
"#;
    let m = extract(src);
    let names: Vec<_> = m.ifdef_macros.iter().map(|im| im.name.as_str()).collect();
    assert!(names.contains(&"FORCED_MDS_KEY"), "names: {names:?}");
    assert!(names.contains(&"PT_OVERRIDDEN_NODES"), "names: {names:?}");
    for im in &m.ifdef_macros {
        assert!(matches!(im.guard_kind, IfdefGuardKind::Test));
        assert!(im.line >= 1);
    }
}

#[test]
fn define_outside_test_block_is_not_recorded() {
    let src = r#"
-module(x).
-define(SOFT_LIMIT, 256).
foo() -> ok.
"#;
    let m = extract(src);
    assert!(m.ifdef_macros.is_empty(), "got: {:?}", m.ifdef_macros);
}

#[test]
fn ifndef_test_else_endif_block_is_recorded_as_variant_c() {
    let src = r#"
-module(x).

-ifndef(TEST).
foo() -> production_body.
-else.
foo() -> test_body.
-endif.
"#;
    let m = extract(src);
    assert_eq!(m.variant_c_blocks.len(), 1, "got: {:?}", m.variant_c_blocks);
    let v = &m.variant_c_blocks[0];
    assert_eq!(v.guard, "TEST");
    assert!(v.start_line < v.end_line);
    assert!(v.else_line.is_some());
    let else_line = v.else_line.unwrap();
    assert!(v.start_line < else_line && else_line < v.end_line);
}

#[test]
fn ifndef_test_without_else_is_recorded() {
    let src = r#"
-module(x).
-ifndef(TEST).
production_only() -> ok.
-endif.
"#;
    let m = extract(src);
    assert_eq!(m.variant_c_blocks.len(), 1);
    assert!(m.variant_c_blocks[0].else_line.is_none());
}

#[test]
fn ifndef_non_test_does_not_emit_variant_c() {
    let src = r#"
-module(x).
-ifndef(OTHER_FLAG).
foo() -> ok.
-endif.
"#;
    let m = extract(src);
    assert!(m.variant_c_blocks.is_empty());
}

#[test]
fn nested_ifndef_test_inside_ifdef_is_recorded() {
    let src = r#"
-module(x).
-ifdef(OTHER).
-ifndef(TEST).
foo() -> prod.
-else.
foo() -> test.
-endif.
-endif.
"#;
    let m = extract(src);
    assert_eq!(m.variant_c_blocks.len(), 1);
    let v = &m.variant_c_blocks[0];
    assert_eq!(v.guard, "TEST");
}
