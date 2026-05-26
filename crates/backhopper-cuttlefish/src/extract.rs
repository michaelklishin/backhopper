// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Extract MFA call references from Cuttlefish fragment bodies.

use std::path::PathBuf;

use backhopper_core::compat::call_sites::extract_into;
use backhopper_core::model::symbol::{Reference, SymbolRef};

use crate::fragments::CuttlefishFragment;

/// Walk each line of `fragment.erlang_body` and collect every MFA reference
/// the existing Erlang call-site extractor recognises, tagged with the
/// fragment's source location.
pub fn extract_references(fragment: &CuttlefishFragment) -> Vec<Reference> {
    let Some(body) = fragment.erlang_body.as_deref() else {
        return Vec::new();
    };
    let path: PathBuf = fragment.source_path.clone();
    let base_line = fragment.body_start_line;
    let mut out: Vec<Reference> = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let mut syms: Vec<SymbolRef> = Vec::new();
        extract_into(line, &mut syms);
        for sym in syms {
            out.push(Reference {
                symbol: sym,
                from_path: path.clone(),
                line: base_line + idx,
            });
        }
    }
    out
}
