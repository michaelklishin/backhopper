// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonicalization: total ordering over modules, headers, and entries.

use std::cmp::Ordering;

use crate::model::snapshot::{ArityMatch, Deprecation, HrlFile, Module};

pub fn canonicalize(modules: &mut Vec<Module>, headers: &mut Vec<HrlFile>) {
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    headers.sort_by(|a, b| a.path.cmp(&b.path));
    modules.dedup_by(|a, b| a.name == b.name);
    headers.dedup_by(|a, b| a.path == b.path);
    for m in modules.iter_mut() {
        m.behaviours.sort();
        m.exports
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.export_types
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.callbacks
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.optional_callbacks
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.specs
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.types
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.opaques
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        m.deprecations
            .sort_by(|a, b| a.function.cmp(&b.function).then(arity_match_cmp(a, b)));
        m.records.sort_by(|a, b| a.name.cmp(&b.name));
        m.records.dedup_by(|a, b| a.name == b.name);
        m.test_only_exports
            .sort_by(|a, b| a.function.cmp(&b.function).then(a.arity.cmp(&b.arity)));
        m.test_only_exports
            .dedup_by(|a, b| a.function == b.function && a.arity == b.arity);
        m.ifdef_macros
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.line.cmp(&b.line)));
        m.ifdef_macros
            .dedup_by(|a, b| a.name == b.name && a.line == b.line);
        m.variant_c_blocks
            .sort_by(|a, b| a.start_line.cmp(&b.start_line).then(a.guard.cmp(&b.guard)));
        m.variant_c_blocks
            .dedup_by(|a, b| a.start_line == b.start_line && a.guard == b.guard);
        m.behaviours.dedup();
        m.exports
            .dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
        m.export_types
            .dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
        m.optional_callbacks
            .dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
        m.opaques
            .dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
    }
    for h in headers.iter_mut() {
        h.types
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        h.opaques
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        h.records.sort_by(|a, b| a.name.cmp(&b.name));
        h.opaques
            .dedup_by(|a, b| a.name == b.name && a.arity == b.arity);
    }
}

fn arity_match_cmp(a: &Deprecation, b: &Deprecation) -> Ordering {
    match (&a.arity_match, &b.arity_match) {
        (ArityMatch::Any, ArityMatch::Any) => Ordering::Equal,
        (ArityMatch::Any, ArityMatch::Exact { .. }) => Ordering::Less,
        (ArityMatch::Exact { .. }, ArityMatch::Any) => Ordering::Greater,
        (ArityMatch::Exact { arity: x }, ArityMatch::Exact { arity: y }) => x.cmp(y),
    }
}
