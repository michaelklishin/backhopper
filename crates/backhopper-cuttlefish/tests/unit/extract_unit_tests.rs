// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_core::model::symbol::SymbolKind;
use backhopper_cuttlefish::{FragmentKind, extract_references, parse_schema};

const OAUTH2_FIXTURE: &str = include_str!("../fixtures/oauth2.schema");

#[test]
fn extracts_mfa_reference_with_schema_source_location() {
    let frags = parse_schema(OAUTH2_FIXTURE, Path::new("oauth2.schema")).unwrap();
    let t = frags
        .iter()
        .find(|f| f.kind == FragmentKind::Translation)
        .unwrap();
    let refs = extract_references(t);
    assert!(!refs.is_empty(), "expected at least one MFA reference");
    let r = refs
        .iter()
        .find(|r| match &r.symbol.kind {
            SymbolKind::Function { mfa } => {
                mfa.module.as_str() == "rabbit_cuttlefish"
                    && mfa.function.as_str() == "optionally_tagged_string"
            }
            _ => false,
        })
        .expect("MFA was extracted");
    assert_eq!(r.from_path, Path::new("oauth2.schema"));
    // The call sits on line 6 of the fixture, one line below `fun(Conf) ->`.
    assert_eq!(r.line, 6);
}

#[test]
fn extracts_no_references_from_mapping_only_schema() {
    let body = r#"
{mapping, "k", "v", []}.
"#;
    let frags = parse_schema(body, Path::new("m.schema")).unwrap();
    assert!(extract_references(&frags[0]).is_empty());
}
