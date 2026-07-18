// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::ModuleName;
use backhopper_xref_graph::{Relation, Vertex, VertexSet};
use proptest::prelude::*;

fn vertex_strategy() -> impl Strategy<Value = Vertex> {
    "[a-z]{1,4}".prop_map(|s| Vertex::Module(ModuleName::new(s).unwrap()))
}

fn relation_strategy() -> impl Strategy<Value = Relation> {
    proptest::collection::vec((vertex_strategy(), vertex_strategy()), 0..15)
        .prop_map(|edges| edges.into_iter().collect())
}

fn vertex_set_strategy() -> impl Strategy<Value = VertexSet> {
    proptest::collection::vec(vertex_strategy(), 0..10).prop_map(|vs| vs.into_iter().collect())
}

proptest! {
    #[test]
    fn union_is_commutative(a in relation_strategy(), b in relation_strategy()) {
        prop_assert_eq!(a.union(&b), b.union(&a));
    }

    #[test]
    fn intersection_is_commutative(a in relation_strategy(), b in relation_strategy()) {
        prop_assert_eq!(a.intersection(&b), b.intersection(&a));
    }

    #[test]
    fn union_is_associative(
        a in relation_strategy(),
        b in relation_strategy(),
        c in relation_strategy()
    ) {
        prop_assert_eq!(a.union(&b).union(&c), a.union(&b.union(&c)));
    }

    #[test]
    fn intersection_distributes_over_union(
        a in relation_strategy(),
        b in relation_strategy(),
        c in relation_strategy()
    ) {
        let lhs = a.intersection(&b.union(&c));
        let rhs = a.intersection(&b).union(&a.intersection(&c));
        prop_assert_eq!(lhs, rhs);
    }

    #[test]
    fn difference_self_is_empty(a in relation_strategy()) {
        prop_assert!(a.difference(&a).is_empty());
    }

    #[test]
    fn reversed_is_involutive(a in relation_strategy()) {
        prop_assert_eq!(a.reversed().reversed(), a);
    }

    #[test]
    fn transitive_closure_is_idempotent(a in relation_strategy()) {
        let once = a.transitive_closure();
        let twice = once.transitive_closure();
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn image_subset_of_targets(a in relation_strategy(), s in vertex_set_strategy()) {
        let img = a.image(&s);
        for v in img.iter() {
            prop_assert!(a.targets().contains(v));
        }
    }

    #[test]
    fn intersection_is_associative(
        a in relation_strategy(),
        b in relation_strategy(),
        c in relation_strategy()
    ) {
        prop_assert_eq!(
            a.intersection(&b).intersection(&c),
            a.intersection(&b.intersection(&c))
        );
    }

    #[test]
    fn preimage_subset_of_sources(a in relation_strategy(), s in vertex_set_strategy()) {
        let pre = a.preimage(&s);
        for v in pre.iter() {
            prop_assert!(a.iter().any(|(src, _)| src == v));
        }
    }

    #[test]
    fn transitive_closure_contains_original(a in relation_strategy()) {
        let tc = a.transitive_closure();
        for (s, t) in a.iter() {
            prop_assert!(tc.contains(s, t));
        }
    }

    #[test]
    fn union_is_idempotent(a in relation_strategy()) {
        prop_assert_eq!(a.union(&a), a);
    }

    #[test]
    fn intersection_is_idempotent(a in relation_strategy()) {
        prop_assert_eq!(a.intersection(&a), a);
    }

    #[test]
    fn vertex_set_union_is_idempotent(a in vertex_set_strategy()) {
        prop_assert_eq!(a.union(&a), a);
    }

    #[test]
    fn vertex_set_intersection_is_idempotent(a in vertex_set_strategy()) {
        prop_assert_eq!(a.intersection(&a), a);
    }

    #[test]
    fn targets_cover_all_edge_targets(a in relation_strategy()) {
        let t = a.targets();
        for (_, tgt) in a.iter() {
            prop_assert!(t.contains(tgt));
        }
    }
}
