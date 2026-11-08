// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonicalization: total ordering over modules, headers, and entries.

use std::cmp::Ordering;
use std::fmt::Display;

use crate::model::snapshot::{ArityMatch, Deprecation, HeaderRef, HrlFile, Module, SnapshotParts};

/// Sort by `key`, then drop entries with an equal key (keeping the
/// first). One helper for the repeated sort-then-dedup pairs; the sort
/// is stable, so ties keep input order.
fn sort_dedup_by<T, K: Ord>(v: &mut Vec<T>, mut key: impl FnMut(&T) -> K) {
    v.sort_by_key(&mut key);
    v.dedup_by(|a, b| key(a) == key(b));
}

pub(crate) fn canonicalize_parts(parts: &mut SnapshotParts) {
    sort_dedup_by(&mut parts.apps, |a| a.name.clone());
    sort_dedup_by(&mut parts.schemas, |s| s.path.clone());
    sort_dedup_by(&mut parts.partials, |p| p.path.clone());
    canonicalize(&mut parts.modules, &mut parts.headers);
    parts.header.apps_scanned.sort();
    parts.header.apps_scanned.dedup();
    parts.header.dep_pins.sort_by(|a, b| a.name.cmp(&b.name));
    parts.header.dep_pins.dedup_by(|a, b| a.name == b.name);
    if let Some(host) = parts.header.host_facts.as_mut() {
        host.release_roots.sort_dedup();
    }
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
        // deprecations use a custom arity-match order, not a simple key, so they stay hand-written
        m.deprecations
            .sort_by(|a, b| a.function.cmp(&b.function).then(arity_match_cmp(a, b)));
        m.deprecations
            .dedup_by(|a, b| a.function == b.function && arity_match_cmp(a, b).is_eq());
    }
    for h in headers.iter_mut() {
        sort_dedup_by(&mut h.includes, HeaderRef::clone);
        sort_dedup_by(&mut h.types, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut h.opaques, |x| (x.name.clone(), x.arity));
        sort_dedup_by(&mut h.records, |x| x.name.clone());
        sort_dedup_by(&mut h.defines, |x| x.key.clone());
    }
}

/// True when `next` does not sort strictly after `prev`: the parser's
/// incremental check and [`out_of_order`]'s whole-list check share this.
pub fn breaks_order<K: Ord>(prev: Option<&K>, next: &K) -> bool {
    prev.is_some_and(|p| next <= p)
}

/// Walks every top-level section list pairwise, the same comparison the
/// streaming parser makes as it reads each entry, and names the first
/// entry out of place. For readers (such as JSON deserialization) that
/// have no source line to report.
pub(crate) fn out_of_order(parts: &SnapshotParts) -> Option<String> {
    first_out_of_order("apps", &parts.apps, |a| &a.name)
        .or_else(|| first_out_of_order("modules", &parts.modules, |m| &m.name))
        .or_else(|| first_out_of_order("headers", &parts.headers, |h| &h.path))
        .or_else(|| first_out_of_order("schemas", &parts.schemas, |s| &s.path))
        .or_else(|| first_out_of_order("partials", &parts.partials, |p| &p.path))
}

fn first_out_of_order<T, K: Ord + Display>(
    label: &str,
    items: &[T],
    key: impl Fn(&T) -> &K,
) -> Option<String> {
    items
        .windows(2)
        .find(|pair| breaks_order(Some(key(&pair[0])), key(&pair[1])))
        .map(|pair| format!("{label} out of order at {}", key(&pair[1])))
}

pub(crate) fn missing_app(parts: &SnapshotParts) -> Option<String> {
    let module = parts.modules.iter().find(|m| m.app.is_none());
    let header = parts.headers.iter().find(|h| h.app.is_none());
    module
        .map(|m| format!("module {} has no app", m.name))
        .or_else(|| header.map(|h| format!("header {} has no app", h.path)))
}

fn arity_match_cmp(a: &Deprecation, b: &Deprecation) -> Ordering {
    match (&a.arity_match, &b.arity_match) {
        (ArityMatch::Any, ArityMatch::Any) => Ordering::Equal,
        (ArityMatch::Any, ArityMatch::Exact { .. }) => Ordering::Less,
        (ArityMatch::Exact { .. }, ArityMatch::Any) => Ordering::Greater,
        (ArityMatch::Exact { arity: x }, ArityMatch::Exact { arity: y }) => x.cmp(y),
    }
}
