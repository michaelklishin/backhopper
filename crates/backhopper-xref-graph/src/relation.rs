// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Vertex sets and binary relations.
//!
//! The algebra here is what xref-style analyses need: union, intersection,
//! difference, forward and backward image, transitive closure, and edge
//! reversal.
//!
//! Everything is keyed on `Vertex` and stored in `BTreeSet` so iteration
//! order is deterministic.

use std::collections::{BTreeSet, btree_set};
use std::iter::Map;

use serde::{Deserialize, Serialize};

use crate::vertex::Vertex;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VertexSet {
    inner: BTreeSet<Vertex>,
}

impl VertexSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `v`. Returns `true` if it was not already present.
    pub fn insert(&mut self, v: Vertex) -> bool {
        self.inner.insert(v)
    }

    /// True when `v` is in the set.
    pub fn contains(&self, v: &Vertex) -> bool {
        self.inner.contains(v)
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True when there are no elements.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate in canonical (ascending) order.
    pub fn iter(&self) -> impl Iterator<Item = &Vertex> {
        self.inner.iter()
    }

    /// Set union: `{ v | v in self or v in other }`.
    pub fn union(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.union(&other.inner).cloned().collect(),
        }
    }

    /// Set intersection: `{ v | v in self and v in other }`.
    pub fn intersection(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.intersection(&other.inner).cloned().collect(),
        }
    }

    /// Set difference: `{ v | v in self and v not in other }`.
    pub fn difference(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.difference(&other.inner).cloned().collect(),
        }
    }
}

impl FromIterator<Vertex> for VertexSet {
    fn from_iter<I: IntoIterator<Item = Vertex>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl<'a> IntoIterator for &'a VertexSet {
    type Item = &'a Vertex;
    type IntoIter = btree_set::Iter<'a, Vertex>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Relation {
    inner: BTreeSet<(Vertex, Vertex)>,
}

impl Relation {
    /// An empty relation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert the edge `(src, tgt)`. Returns `true` if it was not already present.
    pub fn insert(&mut self, src: Vertex, tgt: Vertex) -> bool {
        self.inner.insert((src, tgt))
    }

    /// True when `(src, tgt)` is in the relation.
    pub fn contains(&self, src: &Vertex, tgt: &Vertex) -> bool {
        self.inner.contains(&(src.clone(), tgt.clone()))
    }

    /// Number of edges.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True when there are no edges.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate edges in canonical (lexicographic) order.
    pub fn iter(&self) -> impl Iterator<Item = (&Vertex, &Vertex)> {
        self.inner.iter().map(|(a, b)| (a, b))
    }

    /// Edge set union.
    pub fn union(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.union(&other.inner).cloned().collect(),
        }
    }

    /// Edge set intersection.
    pub fn intersection(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.intersection(&other.inner).cloned().collect(),
        }
    }

    /// Edge set difference.
    pub fn difference(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.difference(&other.inner).cloned().collect(),
        }
    }

    /// The set of vertices that appear as a target on some edge.
    pub fn targets(&self) -> VertexSet {
        self.inner.iter().map(|(_, t)| t.clone()).collect()
    }

    /// Forward image: `{ y | exists x in sources, (x, y) in self }`.
    pub fn image(&self, sources: &VertexSet) -> VertexSet {
        self.inner
            .iter()
            .filter(|(s, _)| sources.contains(s))
            .map(|(_, t)| t.clone())
            .collect()
    }

    /// Backward image: `{ x | exists y in targets, (x, y) in self }`.
    pub fn preimage(&self, targets: &VertexSet) -> VertexSet {
        self.inner
            .iter()
            .filter(|(_, t)| targets.contains(t))
            .map(|(s, _)| s.clone())
            .collect()
    }

    /// Reverse every edge: maps `(s, t)` to `(t, s)`.
    pub fn reversed(&self) -> Relation {
        Relation {
            inner: self
                .inner
                .iter()
                .map(|(s, t)| (t.clone(), s.clone()))
                .collect(),
        }
    }

    /// Irreflexive transitive closure (E+).
    ///
    /// Tarjan's Strongly Connected Component algorithm, then DAG
    /// reachability propagation: O(V + E + sum(component-size^2)).
    pub fn transitive_closure(&self) -> Relation {
        closure::compute(self)
    }
}

impl FromIterator<(Vertex, Vertex)> for Relation {
    fn from_iter<I: IntoIterator<Item = (Vertex, Vertex)>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl<'a> IntoIterator for &'a Relation {
    type Item = (&'a Vertex, &'a Vertex);
    type IntoIter = Map<
        btree_set::Iter<'a, (Vertex, Vertex)>,
        fn(&'a (Vertex, Vertex)) -> (&'a Vertex, &'a Vertex),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter().map(|(a, b)| (a, b))
    }
}

mod closure {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::relation::Relation;
    use crate::vertex::Vertex;

    pub(super) fn compute(r: &Relation) -> Relation {
        let adj = build_adj(r);
        let sccs = tarjan_sccs(&adj);
        let mut comp_of: BTreeMap<&Vertex, usize> = BTreeMap::new();
        for (idx, scc) in sccs.iter().enumerate() {
            for v in scc {
                comp_of.insert(v, idx);
            }
        }
        let mut cond_adj: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); sccs.len()];
        for (s, ts) in &adj {
            let ci = comp_of[s];
            for t in ts {
                let cj = comp_of[t];
                if ci != cj {
                    cond_adj[ci].insert(cj);
                }
            }
        }
        let topo = reverse_topo(&cond_adj);
        let mut reach: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); sccs.len()];
        for &c in &topo {
            let mut rr = BTreeSet::new();
            for &nc in &cond_adj[c] {
                rr.insert(nc);
                rr.extend(reach[nc].iter().copied());
            }
            reach[c] = rr;
        }
        let mut out: BTreeSet<(Vertex, Vertex)> = BTreeSet::new();
        for (idx, scc) in sccs.iter().enumerate() {
            // True iff every vertex in the SCC reaches itself: size > 1, or self-loop.
            let intra_cycle =
                scc.len() > 1 || adj.get(&scc[0]).is_some_and(|out| out.contains(&scc[0]));
            for v in scc {
                if intra_cycle {
                    for u in scc {
                        out.insert(((*v).clone(), (*u).clone()));
                    }
                }
                for &nc in &reach[idx] {
                    for u in &sccs[nc] {
                        out.insert(((*v).clone(), (*u).clone()));
                    }
                }
            }
        }
        Relation { inner: out }
    }

    fn build_adj(r: &Relation) -> BTreeMap<&Vertex, Vec<&Vertex>> {
        let mut adj: BTreeMap<&Vertex, Vec<&Vertex>> = BTreeMap::new();
        for (s, t) in r.iter() {
            adj.entry(s).or_default().push(t);
            adj.entry(t).or_default();
        }
        adj
    }

    // Iterative Tarjan over integer vertex ids, so a deep call graph over
    // the RabbitMQ monorepo cannot overflow the thread stack. Ids index a
    // sorted vertex list, so the per-vertex state is `Vec`-indexed instead
    // of map-keyed, and the SCC output keeps the same vertex ordering the
    // recursive version produced.
    fn tarjan_sccs<'a>(adj: &BTreeMap<&'a Vertex, Vec<&'a Vertex>>) -> Vec<Vec<&'a Vertex>> {
        let nodes: Vec<&'a Vertex> = adj.keys().copied().collect();
        let id_of: BTreeMap<&Vertex, usize> =
            nodes.iter().enumerate().map(|(i, v)| (*v, i)).collect();
        let neighbours: Vec<Vec<usize>> = nodes
            .iter()
            .map(|v| adj[v].iter().map(|w| id_of[w]).collect())
            .collect();
        let n = nodes.len();

        const UNVISITED: usize = usize::MAX;
        let mut indices = vec![UNVISITED; n];
        let mut lowlinks = vec![0usize; n];
        let mut on_stack = vec![false; n];
        let mut tarjan_stack: Vec<usize> = Vec::new();
        let mut next_index = 0usize;
        let mut out: Vec<Vec<&'a Vertex>> = Vec::new();

        // Each frame is (vertex, index of the next child to visit).
        let mut work: Vec<(usize, usize)> = Vec::new();
        for start in 0..n {
            if indices[start] != UNVISITED {
                continue;
            }
            work.push((start, 0));
            while let Some(&(v, child)) = work.last() {
                if child == 0 {
                    indices[v] = next_index;
                    lowlinks[v] = next_index;
                    next_index += 1;
                    tarjan_stack.push(v);
                    on_stack[v] = true;
                }
                let mut recursed = false;
                let mut i = child;
                while i < neighbours[v].len() {
                    let w = neighbours[v][i];
                    if indices[w] == UNVISITED {
                        work.last_mut().unwrap().1 = i + 1;
                        work.push((w, 0));
                        recursed = true;
                        break;
                    }
                    if on_stack[w] {
                        lowlinks[v] = lowlinks[v].min(indices[w]);
                    }
                    i += 1;
                }
                if recursed {
                    continue;
                }
                if lowlinks[v] == indices[v] {
                    let mut scc = Vec::new();
                    loop {
                        let w = tarjan_stack.pop().expect("stack non-empty during SCC pop");
                        on_stack[w] = false;
                        scc.push(nodes[w]);
                        if w == v {
                            break;
                        }
                    }
                    scc.sort();
                    out.push(scc);
                }
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    lowlinks[parent] = lowlinks[parent].min(lowlinks[v]);
                }
            }
        }
        out
    }

    fn reverse_topo(adj: &[BTreeSet<usize>]) -> Vec<usize> {
        let n = adj.len();
        let mut state = vec![0u8; n];
        let mut order = Vec::new();
        for v in 0..n {
            if state[v] == 0 {
                dfs(v, adj, &mut state, &mut order);
            }
        }
        order
    }

    fn dfs(v: usize, adj: &[BTreeSet<usize>], state: &mut [u8], order: &mut Vec<usize>) {
        state[v] = 1;
        for &u in &adj[v] {
            if state[u] == 0 {
                dfs(u, adj, state, order);
            }
        }
        state[v] = 2;
        order.push(v);
    }
}
