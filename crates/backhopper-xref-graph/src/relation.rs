// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Vertex sets and binary relations.
//!
//! The algebra here is what xref-style analyses need: union, intersection,
//! difference, the two restrictions (by source, by target, by both),
//! forward and backward image, composition, transitive closure (irreflexive
//! and reflexive), and edge reversal.
//!
//! Everything is keyed on `Vertex` and stored in `BTreeSet` so iteration
//! order is deterministic.

use std::collections::{BTreeMap, BTreeSet, btree_set};
use std::iter::Map;

use serde::{Deserialize, Serialize};

use crate::vertex::Vertex;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VertexSet {
    inner: BTreeSet<Vertex>,
}

impl VertexSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, v: Vertex) -> bool {
        self.inner.insert(v)
    }

    pub fn contains(&self, v: &Vertex) -> bool {
        self.inner.contains(v)
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Vertex> {
        self.inner.iter()
    }

    pub fn union(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.union(&other.inner).cloned().collect(),
        }
    }

    pub fn intersection(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.intersection(&other.inner).cloned().collect(),
        }
    }

    pub fn difference(&self, other: &VertexSet) -> VertexSet {
        VertexSet {
            inner: self.inner.difference(&other.inner).cloned().collect(),
        }
    }

    /// Complement against an explicit universe.
    pub fn complement(&self, universe: &VertexSet) -> VertexSet {
        universe.difference(self)
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, src: Vertex, tgt: Vertex) -> bool {
        self.inner.insert((src, tgt))
    }

    pub fn contains(&self, src: &Vertex, tgt: &Vertex) -> bool {
        self.inner.contains(&(src.clone(), tgt.clone()))
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Vertex, &Vertex)> {
        self.inner.iter().map(|(a, b)| (a, b))
    }

    pub fn union(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.union(&other.inner).cloned().collect(),
        }
    }

    pub fn intersection(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.intersection(&other.inner).cloned().collect(),
        }
    }

    pub fn difference(&self, other: &Relation) -> Relation {
        Relation {
            inner: self.inner.difference(&other.inner).cloned().collect(),
        }
    }

    pub fn sources(&self) -> VertexSet {
        self.inner.iter().map(|(s, _)| s.clone()).collect()
    }

    pub fn targets(&self) -> VertexSet {
        self.inner.iter().map(|(_, t)| t.clone()).collect()
    }

    pub fn filter_sources_in(&self, sources: &VertexSet) -> Relation {
        Relation {
            inner: self
                .inner
                .iter()
                .filter(|(s, _)| sources.contains(s))
                .cloned()
                .collect(),
        }
    }

    pub fn filter_targets_in(&self, targets: &VertexSet) -> Relation {
        Relation {
            inner: self
                .inner
                .iter()
                .filter(|(_, t)| targets.contains(t))
                .cloned()
                .collect(),
        }
    }

    pub fn filter_both_in(&self, vs: &VertexSet) -> Relation {
        Relation {
            inner: self
                .inner
                .iter()
                .filter(|(s, t)| vs.contains(s) && vs.contains(t))
                .cloned()
                .collect(),
        }
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

    /// Relational composition: `{ (x, z) | exists y, (x, y) in self, (y, z) in other }`.
    pub fn compose(&self, other: &Relation) -> Relation {
        let mut by_source: BTreeMap<&Vertex, Vec<&Vertex>> = BTreeMap::new();
        for (s, t) in other.iter() {
            by_source.entry(s).or_default().push(t);
        }
        let mut out = BTreeSet::new();
        for (x, y) in self.iter() {
            if let Some(zs) = by_source.get(y) {
                for z in zs {
                    out.insert((x.clone(), (*z).clone()));
                }
            }
        }
        Relation { inner: out }
    }

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
    /// Implementation: Tarjan SCC, then DAG reachability propagation.
    /// O(V + E + sum(component-size^2)): far better than naive Floyd-Warshall.
    pub fn transitive_closure(&self) -> Relation {
        closure::compute(self, false)
    }

    /// Reflexive transitive closure (E*).
    pub fn reflexive_transitive_closure(&self) -> Relation {
        closure::compute(self, true)
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

    pub(super) fn compute(r: &Relation, reflexive: bool) -> Relation {
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
                if reflexive {
                    out.insert(((*v).clone(), (*v).clone()));
                }
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

    fn tarjan_sccs<'a>(adj: &BTreeMap<&'a Vertex, Vec<&'a Vertex>>) -> Vec<Vec<&'a Vertex>> {
        let mut index_counter: usize = 0;
        let mut stack: Vec<&'a Vertex> = Vec::new();
        let mut on_stack: BTreeSet<&'a Vertex> = BTreeSet::new();
        let mut indices: BTreeMap<&'a Vertex, usize> = BTreeMap::new();
        let mut lowlinks: BTreeMap<&'a Vertex, usize> = BTreeMap::new();
        let mut out: Vec<Vec<&'a Vertex>> = Vec::new();
        for v in adj.keys() {
            if !indices.contains_key(v) {
                strongconnect(
                    v,
                    adj,
                    &mut index_counter,
                    &mut stack,
                    &mut on_stack,
                    &mut indices,
                    &mut lowlinks,
                    &mut out,
                );
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn strongconnect<'a>(
        v: &'a Vertex,
        adj: &BTreeMap<&'a Vertex, Vec<&'a Vertex>>,
        index_counter: &mut usize,
        stack: &mut Vec<&'a Vertex>,
        on_stack: &mut BTreeSet<&'a Vertex>,
        indices: &mut BTreeMap<&'a Vertex, usize>,
        lowlinks: &mut BTreeMap<&'a Vertex, usize>,
        out: &mut Vec<Vec<&'a Vertex>>,
    ) {
        indices.insert(v, *index_counter);
        lowlinks.insert(v, *index_counter);
        *index_counter += 1;
        stack.push(v);
        on_stack.insert(v);
        if let Some(neighbours) = adj.get(v) {
            for w in neighbours {
                if !indices.contains_key(w) {
                    strongconnect(
                        w,
                        adj,
                        index_counter,
                        stack,
                        on_stack,
                        indices,
                        lowlinks,
                        out,
                    );
                    let wl = lowlinks[w];
                    let vl = lowlinks[v];
                    lowlinks.insert(v, vl.min(wl));
                } else if on_stack.contains(w) {
                    let wi = indices[w];
                    let vl = lowlinks[v];
                    lowlinks.insert(v, vl.min(wi));
                }
            }
        }
        if lowlinks[v] == indices[v] {
            let mut scc = Vec::new();
            loop {
                let w = stack.pop().expect("stack non-empty during SCC pop");
                on_stack.remove(w);
                scc.push(w);
                if w == v {
                    break;
                }
            }
            scc.sort();
            out.push(scc);
        }
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