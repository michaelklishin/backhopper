//! Canonicalization: total ordering over modules, headers, and entries.

use std::cmp::Ordering;

use crate::model::snapshot::{ArityMatch, Deprecation, HrlFile, Module};

pub fn canonicalize(modules: &mut Vec<Module>, headers: &mut Vec<HrlFile>) {
    modules.sort_by(|a, b| a.name.cmp(&b.name));
    headers.sort_by(|a, b| a.path.cmp(&b.path));
    dedupe_consecutive(modules, |a, b| a.name == b.name);
    dedupe_consecutive(headers, |a, b| a.path == b.path);
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
        dedupe_consecutive(&mut m.behaviours, |a, b| a == b);
        dedupe_consecutive(&mut m.exports, |a, b| {
            a.name == b.name && a.arity == b.arity
        });
        dedupe_consecutive(&mut m.export_types, |a, b| {
            a.name == b.name && a.arity == b.arity
        });
        dedupe_consecutive(&mut m.optional_callbacks, |a, b| {
            a.name == b.name && a.arity == b.arity
        });
        dedupe_consecutive(&mut m.opaques, |a, b| {
            a.name == b.name && a.arity == b.arity
        });
    }
    for h in headers.iter_mut() {
        h.types
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        h.opaques
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.arity.cmp(&b.arity)));
        h.records.sort_by(|a, b| a.name.cmp(&b.name));
        dedupe_consecutive(&mut h.opaques, |a, b| {
            a.name == b.name && a.arity == b.arity
        });
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

fn dedupe_consecutive<T, F: Fn(&T, &T) -> bool>(v: &mut Vec<T>, eq: F) {
    let mut i = 1;
    while i < v.len() {
        if eq(&v[i - 1], &v[i]) {
            v.remove(i);
        } else {
            i += 1;
        }
    }
}
