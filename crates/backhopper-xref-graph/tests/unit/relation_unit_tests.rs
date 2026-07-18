// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::ModuleName;
use backhopper_xref_graph::{Relation, Vertex, VertexSet};

fn m(name: &str) -> Vertex {
    Vertex::Module(ModuleName::new(name.to_owned()).unwrap())
}

fn rel(pairs: &[(&str, &str)]) -> Relation {
    let mut r = Relation::new();
    for (a, b) in pairs {
        r.insert(m(a), m(b));
    }
    r
}

fn vs(names: &[&str]) -> VertexSet {
    names.iter().map(|n| m(n)).collect()
}

#[test]
fn union_empty_is_identity() {
    let a = rel(&[("a", "b")]);
    let e = Relation::new();
    assert_eq!(a.union(&e), a);
    assert_eq!(e.union(&a), a);
}

#[test]
fn intersection_disjoint_is_empty() {
    let a = rel(&[("a", "b")]);
    let b = rel(&[("c", "d")]);
    assert!(a.intersection(&b).is_empty());
}

#[test]
fn intersection_self_is_self() {
    let a = rel(&[("a", "b"), ("c", "d")]);
    assert_eq!(a.intersection(&a), a);
}

#[test]
fn difference_self_is_empty() {
    let a = rel(&[("a", "b")]);
    assert!(a.difference(&a).is_empty());
}

#[test]
fn targets_is_the_target_set_projection() {
    let a = rel(&[("a", "b"), ("a", "c"), ("d", "b")]);
    assert_eq!(a.targets(), vs(&["b", "c"]));
}

#[test]
fn image_is_forward_reachable_in_one_step() {
    let a = rel(&[("a", "b"), ("a", "c"), ("x", "y")]);
    assert_eq!(a.image(&vs(&["a"])), vs(&["b", "c"]));
}

#[test]
fn preimage_is_backward_reachable_in_one_step() {
    let a = rel(&[("a", "b"), ("c", "b")]);
    assert_eq!(a.preimage(&vs(&["b"])), vs(&["a", "c"]));
}

#[test]
fn reversed_swaps_endpoints() {
    let r = rel(&[("a", "b"), ("c", "d")]);
    assert_eq!(r.reversed(), rel(&[("b", "a"), ("d", "c")]));
}

#[test]
fn reversed_is_involutive() {
    let r = rel(&[("a", "b"), ("c", "d"), ("a", "c")]);
    assert_eq!(r.reversed().reversed(), r);
}

#[test]
fn transitive_closure_chain_extends_to_all_pairs() {
    let r = rel(&[("a", "b"), ("b", "c"), ("c", "d")]);
    let tc = r.transitive_closure();
    assert!(tc.contains(&m("a"), &m("c")));
    assert!(tc.contains(&m("a"), &m("d")));
    assert!(tc.contains(&m("b"), &m("d")));
    // No reflexive self-loops on an acyclic graph in irreflexive closure.
    assert!(!tc.contains(&m("a"), &m("a")));
}

#[test]
fn transitive_closure_merges_converging_paths_in_a_diamond() {
    let r = rel(&[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")]);
    let tc = r.transitive_closure();
    assert!(tc.contains(&m("a"), &m("d")));
    assert!(tc.contains(&m("b"), &m("d")));
    assert!(tc.contains(&m("c"), &m("d")));
    // a shared descendant reached by two paths appears once, acyclic
    assert!(!tc.contains(&m("a"), &m("a")));
    assert!(!tc.contains(&m("d"), &m("a")));
}

#[test]
fn transitive_closure_keeps_two_disjoint_cycles_separate() {
    let r = rel(&[("a", "b"), ("b", "a"), ("c", "d"), ("d", "c")]);
    let tc = r.transitive_closure();
    for v in ["a", "b", "c", "d"] {
        assert!(tc.contains(&m(v), &m(v)), "{v} should reach itself");
    }
    assert!(tc.contains(&m("a"), &m("b")));
    assert!(tc.contains(&m("c"), &m("d")));
    // the two strongly connected components never reach each other
    assert!(!tc.contains(&m("a"), &m("c")));
    assert!(!tc.contains(&m("d"), &m("b")));
}

#[test]
fn transitive_closure_is_idempotent() {
    let r = rel(&[("a", "b"), ("b", "c"), ("c", "d")]);
    let once = r.transitive_closure();
    let twice = once.transitive_closure();
    assert_eq!(once, twice);
}

#[test]
fn transitive_closure_finds_cycle_self_edges() {
    let r = rel(&[("a", "b"), ("b", "c"), ("c", "a")]);
    let tc = r.transitive_closure();
    assert!(tc.contains(&m("a"), &m("a")));
    assert!(tc.contains(&m("a"), &m("b")));
    assert!(tc.contains(&m("a"), &m("c")));
}

#[test]
fn image_of_empty_set_is_empty() {
    let r = rel(&[("a", "b"), ("c", "d")]);
    assert!(r.image(&vs(&[])).is_empty());
}

#[test]
fn preimage_of_empty_set_is_empty() {
    let r = rel(&[("a", "b"), ("c", "d")]);
    assert!(r.preimage(&vs(&[])).is_empty());
}

#[test]
fn transitive_closure_self_loop_produces_self_edge_in_irreflexive() {
    let r = rel(&[("a", "a")]);
    let tc = r.transitive_closure();
    assert!(tc.contains(&m("a"), &m("a")));
}

#[test]
fn relation_len_matches_edge_count() {
    let r = rel(&[("a", "b"), ("c", "d"), ("a", "c")]);
    assert_eq!(r.len(), 3);
    assert!(!r.is_empty());
}

#[test]
fn empty_relation_is_empty_and_has_zero_len() {
    let r = Relation::new();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn vertex_set_len_and_is_empty() {
    let s = vs(&["a", "b", "c"]);
    assert_eq!(s.len(), 3);
    assert!(!s.is_empty());
    assert!(VertexSet::new().is_empty());
}

#[test]
fn relation_iteration_order_is_sorted() {
    let r = rel(&[("z", "a"), ("a", "z"), ("m", "m")]);
    let edges: Vec<(String, String)> = r
        .iter()
        .map(|(s, t)| (s.to_string(), t.to_string()))
        .collect();
    let mut sorted = edges.clone();
    sorted.sort();
    assert_eq!(edges, sorted);
}

// A deep chain would overflow the thread stack under a per-edge recursive
// SCC pass; the iterative pass handles it. Kept modest so the O(n^2)
// closure stays cheap while the SCC recursion depth is still 800.
#[test]
fn transitive_closure_handles_a_deep_chain_without_overflow() {
    let mut r = Relation::new();
    let names: Vec<String> = (0..800).map(|i| format!("m{i:04}")).collect();
    for pair in names.windows(2) {
        r.insert(m(&pair[0]), m(&pair[1]));
    }
    let tc = r.transitive_closure();
    let first = m(&names[0]);
    let last = m(&names[names.len() - 1]);
    assert!(tc.contains(&first, &last));
}
