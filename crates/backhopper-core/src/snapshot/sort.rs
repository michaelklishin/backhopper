// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonicalization: total ordering over modules, headers, and entries.

use std::cmp::Ordering;

use crate::model::snapshot::{ArityMatch, Deprecation, HrlFile, Module};

/// Sort by `key`, then drop entries with an equal key (keeping the
/// first). One helper for the repeated sort-then-dedup pairs; the sort
/// is stable, so ties keep input order.
fn sort_dedup_by<T, K: Ord>(v: &mut Vec<T>, mut key: impl FnMut(&T) -> K) {
    v.sort_by_key(&mut key);
    v.dedup_by(|a, b| key(a) == key(b));
}

pub fn canonicalize(modules: &mut Vec<Module>, headers: &mut Vec<HrlFile>) {
    sort_dedup_by(modules, |m| m.name.clone());
    sort_dedup_by(headers, |h| h.path.clone());
    for m in modules.iter_mut() {
        m.behaviours.sort();
        m.behaviours.dedup();
        sort_dedup_by(&mut m.exports, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.export_types, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.callbacks, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.optional_callbacks, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.specs, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.types, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.opaques, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut m.records, |x| x.name.clone());
        sort_dedup_by(&mut m.test_only_exports, |x| (x.function.clone(), x.arity));
        sort_dedup_by(&mut m.ifdef_macros, |x| (x.name.clone(), x.line));
        sort_dedup_by(&mut m.variant_c_blocks, |x| (x.start_line, x.guard.clone()));
        sort_dedup_by(&mut m.wire_constants, |x| x.macro_name.clone());
        // deprecations sort and dedup by a custom arity-match order, not
        // a simple key, so they stay hand-written
        m.deprecations
            .sort_by(|a, b| a.function.cmp(&b.function).then(arity_match_cmp(a, b)));
        m.deprecations
            .dedup_by(|a, b| a.function == b.function && arity_match_cmp(a, b).is_eq());
    }
    for h in headers.iter_mut() {
        sort_dedup_by(&mut h.types, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut h.opaques, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut h.records, |x| x.name.clone());
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
