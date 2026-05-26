// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_core::model::verdict::{FileKind, InapplicableReason, TouchedKinds};

#[test]
fn classify_recognizes_erl_files() {
    assert_eq!(
        TouchedKinds::classify(Path::new("src/rabbit.erl")),
        FileKind::Erl
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("apps/rabbit/src/rabbit_amqqueue.erl")),
        FileKind::Erl
    );
}

#[test]
fn classify_recognizes_hrl_files() {
    assert_eq!(
        TouchedKinds::classify(Path::new("include/rabbit_framing.hrl")),
        FileKind::Hrl
    );
}

#[test]
fn classify_recognizes_schema_and_snippets() {
    assert_eq!(
        TouchedKinds::classify(Path::new("priv/schema/rabbit.schema")),
        FileKind::Schema
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("test/config_schema_SUITE_data/a.snippets")),
        FileKind::Tests,
        "_SUITE_data wins over .snippets: it's still a test fixture"
    );
}

#[test]
fn classify_recognizes_docs() {
    assert_eq!(
        TouchedKinds::classify(Path::new("README.md")),
        FileKind::Docs
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("docs/operations.md")),
        FileKind::Docs
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("CHANGELOG.adoc")),
        FileKind::Docs
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("notes.txt")),
        FileKind::Docs
    );
}

#[test]
fn classify_recognizes_tests() {
    assert_eq!(
        TouchedKinds::classify(Path::new("test/rabbit_SUITE.erl")),
        FileKind::Tests
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("tests/integration/foo.erl")),
        FileKind::Tests
    );
}

#[test]
fn classify_falls_through_to_other_for_unknown() {
    assert_eq!(
        TouchedKinds::classify(Path::new("Makefile")),
        FileKind::Other
    );
    assert_eq!(
        TouchedKinds::classify(Path::new("Cargo.toml")),
        FileKind::Other
    );
}

#[test]
fn from_paths_tallies_each_kind() {
    let paths = [
        "src/rabbit.erl",
        "include/x.hrl",
        "include/y.hrl",
        "README.md",
        "Makefile",
        "priv/schema/foo.schema",
        "test/bar_SUITE.erl",
    ];
    let tk = TouchedKinds::from_paths(paths);
    assert_eq!(tk.erl, 1);
    assert_eq!(tk.hrl, 2);
    assert_eq!(tk.docs, 1);
    assert_eq!(tk.other, 1);
    assert_eq!(tk.schema, 1);
    assert_eq!(tk.tests, 1);
}

#[test]
fn inapplicable_reason_is_none_when_any_erl_or_hrl_touched() {
    let tk = TouchedKinds {
        erl: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(tk.inapplicable_reason(), None);

    let tk = TouchedKinds {
        hrl: 1,
        docs: 5,
        ..TouchedKinds::default()
    };
    assert_eq!(tk.inapplicable_reason(), None);
}

#[test]
fn inapplicable_reason_distinguishes_docs_tests_schema() {
    let tk = TouchedKinds {
        docs: 3,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlyDocsTouched)
    );

    let tk = TouchedKinds {
        tests: 2,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlyTestFixturesTouched)
    );

    let tk = TouchedKinds {
        schema: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::OnlySchemaTouched)
    );
}

#[test]
fn inapplicable_reason_for_mixed_non_surface_is_generic() {
    let tk = TouchedKinds {
        docs: 1,
        tests: 1,
        ..TouchedKinds::default()
    };
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::NoErlangSurfaceTouched)
    );
}

#[test]
fn empty_touched_kinds_is_inapplicable_no_surface() {
    let tk = TouchedKinds::default();
    assert!(tk.is_empty());
    assert_eq!(
        tk.inapplicable_reason(),
        Some(InapplicableReason::NoErlangSurfaceTouched)
    );
}
