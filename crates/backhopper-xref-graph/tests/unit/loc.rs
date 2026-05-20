// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::path::Path;

use backhopper_xref_graph::loc::PathInterner;
use backhopper_xref_graph::{Loc, PathId, Position};

#[test]
fn position_zero_is_one_indexed() {
    let p = Position::zero();
    assert_eq!(p.line, 1);
    assert_eq!(p.column, 1);
    assert_eq!(p.byte_offset, 0);
}

#[test]
fn loc_point_collapses_start_and_end() {
    let l = Loc::point(PathId::new(0), Position::new(3, 7, 42));
    assert_eq!(l.start, l.end);
}

#[test]
fn interner_handles_many_paths_without_collision() {
    let mut p = PathInterner::new();
    let mut ids = Vec::new();
    for i in 0..1000 {
        let path_str = format!("/tmp/file_{}.erl", i);
        ids.push(p.intern(Path::new(&path_str)));
    }
    assert_eq!(p.len(), 1000);
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 1000);
}

#[test]
fn interner_is_empty_starts_empty() {
    let p = PathInterner::new();
    assert!(p.is_empty());
}

#[test]
fn loc_span_preserves_distinct_start_and_end() {
    let start = Position::new(1, 1, 0);
    let end = Position::new(3, 5, 42);
    let l = Loc::span(PathId::new(0), start, end);
    assert_eq!(l.start, start);
    assert_eq!(l.end, end);
    assert_ne!(l.start, l.end);
}

#[test]
fn loc_with_expansion_chains() {
    let outer = Loc::point(PathId::new(0), Position::new(1, 1, 0));
    let inner = Loc::point(PathId::new(1), Position::new(5, 5, 50)).with_expansion(outer.clone());
    assert_eq!(inner.expanded_from.as_deref(), Some(&outer));
}

#[test]
fn loc_expansion_chain_supports_two_levels() {
    let a = Loc::point(PathId::new(0), Position::new(1, 1, 0));
    let b = Loc::point(PathId::new(1), Position::new(2, 2, 5)).with_expansion(a);
    let c = Loc::point(PathId::new(2), Position::new(3, 3, 10)).with_expansion(b);
    let inner = c.expanded_from.unwrap();
    let outer = inner.expanded_from.unwrap();
    assert_eq!(outer.start, Position::new(1, 1, 0));
}

#[test]
fn interner_assigns_sequential_ids() {
    let mut p = PathInterner::new();
    let a = p.intern(Path::new("/a"));
    let b = p.intern(Path::new("/b"));
    let a2 = p.intern(Path::new("/a"));
    assert_eq!(a, a2);
    assert_ne!(a, b);
    assert_eq!(p.len(), 2);
}

#[test]
fn interner_round_trips_paths() {
    let mut p = PathInterner::new();
    let id = p.intern(Path::new("/x/y"));
    assert_eq!(p.get(id).unwrap(), Path::new("/x/y"));
    assert!(p.get(PathId::new(999)).is_none());
}

#[test]
fn loc_orders_by_path_then_start() {
    let a = Loc::point(PathId::new(0), Position::new(1, 1, 0));
    let b = Loc::point(PathId::new(0), Position::new(2, 1, 5));
    let c = Loc::point(PathId::new(1), Position::new(1, 1, 0));
    let mut v = [c.clone(), b.clone(), a.clone()];
    v.sort();
    assert_eq!(v, [a, b, c]);
}