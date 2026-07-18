// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `Reason::is_path_scoped` decides which reasons path routing may
//! drop for a pin that owns none of the touched paths. The
//! compile-time exhaustiveness is in the crate itself (the enum is
//! `#[non_exhaustive]`, so external matches need a wildcard); this
//! table pins the classification of representative instances.

use std::path::PathBuf;
use std::str::FromStr;

use backhopper_core::model::names::{Arity, FunctionName, Mfa, ModuleName, RelativePath, TypeName};
use backhopper_core::model::symbol::SymbolRef;
use backhopper_core::model::verdict::{Reason, TranslationSource};

fn expected_path_scoped(reason: &Reason) -> bool {
    matches!(
        reason,
        Reason::FileAbsent { .. }
            | Reason::ContextDrift { .. }
            | Reason::PreimageDrifted { .. }
            | Reason::PreimageMissing { .. }
            | Reason::ModuleRelocated { .. }
            | Reason::PathRename { .. }
            | Reason::TargetPathAbsent { .. }
            | Reason::UnsupportedFileType { .. }
            | Reason::SyntacticArtifact { .. }
    )
}

fn module() -> ModuleName {
    ModuleName::new("m").unwrap()
}

fn function() -> FunctionName {
    FunctionName::new("f").unwrap()
}

fn samples() -> Vec<Reason> {
    vec![
        Reason::FileAbsent {
            path: PathBuf::from("src/m.erl"),
        },
        Reason::ContextDrift {
            path: PathBuf::from("src/m.erl"),
            hunk_index: 0,
        },
        Reason::PreimageDrifted {
            path: PathBuf::from("src/m.erl"),
            hunk_index: 0,
            line_delta: 3,
        },
        Reason::PreimageMissing {
            path: PathBuf::from("src/m.erl"),
            hunk_index: 0,
            preimage_excerpt: "x".into(),
        },
        Reason::ModuleRelocated {
            module: module(),
            patch_path: PathBuf::from("src/m.erl"),
        },
        Reason::PathRename {
            source_path: PathBuf::from("a"),
            target_path: PathBuf::from("b"),
            translation: TranslationSource::ConfigStanza { name: "t".into() },
        },
        Reason::TargetPathAbsent {
            path: RelativePath::new("deps/rabbit/src/m.erl").unwrap(),
        },
        Reason::UnsupportedFileType {
            path: PathBuf::from("lib/m.ex"),
        },
        Reason::MissingSymbol {
            symbol: SymbolRef::function(Mfa::from_str("m:f/1").unwrap()),
            first_seen_at_tag: None,
            needs_pin_at_least: None,
            suggested_replacement: None,
        },
        Reason::ArityChanged {
            module: module(),
            function: function(),
            expected: Arity::new(2),
            found: vec![Arity::new(1)],
            expected_available_at: None,
            needs_pin_at_least: None,
        },
        Reason::SignatureChanged {
            module: module(),
            function: function(),
            arity: Arity::new(1),
            expected_spec: "a".into(),
            found_spec: "b".into(),
        },
        Reason::NowHidden { module: module() },
        Reason::UntrackedModuleMissing { module: module() },
        Reason::MissingType {
            module: module(),
            name: TypeName::new("t").unwrap(),
            arity: Arity::new(0),
        },
        Reason::WireContractRegression {
            module: module(),
            pin_version: 2,
            patch_version: 1,
        },
        Reason::QualifiedCallUndefinedOnTarget {
            source_path: RelativePath::new("deps/rabbit/src/m.erl").unwrap(),
            module: module(),
            function: function(),
            arity: Arity::new(1),
            line: 1,
        },
        Reason::QualifiedCallReturnShapeDrift {
            source_path: RelativePath::new("deps/rabbit/src/m.erl").unwrap(),
            module: module(),
            function: function(),
            arity: Arity::new(1),
            source_signature: "f(term()) -> binary()".into(),
            target_signature: "f(term()) -> list()".into(),
            line: 1,
        },
    ]
}

#[test]
fn classification_matches_the_table() {
    for reason in samples() {
        assert_eq!(
            reason.is_path_scoped(),
            expected_path_scoped(&reason),
            "classification drifted for {reason:?}"
        );
    }
}

// Every non-blocking path-scoped reason must be droppable, or routing would resurrect noise.
#[test]
fn path_scoped_family_covers_the_preimage_reasons() {
    let preimage = Reason::PreimageMissing {
        path: PathBuf::from("src/m.erl"),
        hunk_index: 0,
        preimage_excerpt: "x".into(),
    };
    assert!(preimage.is_path_scoped());
}
