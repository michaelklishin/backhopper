// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! The symbol-availability probe: answers "at which snapshot tag
//! strictly later than the pin does a missing symbol first appear?"
//! and reclassifies the affected reasons as adaptable (land the dep
//! pin bump first).
//!
//! Runs after the verdict cache on hits and misses alike: its input
//! is the project's snapshot *tag set*, which is not a cache-key
//! input, so cached values stay unclassified on disk.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use backhopper_core::config::Config;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::pin::Pin;
use backhopper_core::model::snapshot::{FunArity, Snapshot, state};
use backhopper_core::model::verdict::{AvailabilityQuery, SeriesEvaluation};
use backhopper_core::store::{ReadOnly, SnapshotStore};
use backhopper_core::versions::version_cmp;

use crate::commands::snapshot_cache::SnapshotCache;

type MemoKey = (ProjectName, TagName, AvailabilityQuery);

#[derive(Debug)]
pub struct AvailabilityProbe<'a> {
    cfg: &'a Config,
    store: &'a SnapshotStore<ReadOnly>,
    cache: &'a SnapshotCache<'a>,
    memo: BTreeMap<MemoKey, Option<TagName>>,
    // Listing tags reads the snapshot directory; one scan per project.
    tags: BTreeMap<ProjectName, Vec<TagName>>,
}

impl<'a> AvailabilityProbe<'a> {
    pub fn new(
        cfg: &'a Config,
        store: &'a SnapshotStore<ReadOnly>,
        cache: &'a SnapshotCache<'a>,
    ) -> Self {
        Self {
            cfg,
            store,
            cache,
            memo: BTreeMap::new(),
            tags: BTreeMap::new(),
        }
    }

    pub fn apply(&mut self, evaluation: &mut SeriesEvaluation) {
        evaluation.apply_symbol_availability(|pin, query| self.first_later_tag(pin, query));
    }

    fn first_later_tag(&mut self, pin: &Pin, query: &AvailabilityQuery) -> Option<TagName> {
        // Self pins have their own promotion path (MissingPrereq).
        let project = self.cfg.project(&pin.project).ok()?;
        if project.is_self() {
            return None;
        }
        let key = (pin.project.clone(), pin.tag.clone(), query.clone());
        if let Some(memoized) = self.memo.get(&key) {
            return memoized.clone();
        }
        let result = self.compute(pin, query);
        self.memo.insert(key, result.clone());
        result
    }

    fn compute(&mut self, pin: &Pin, query: &AvailabilityQuery) -> Option<TagName> {
        self.ensure_tags(&pin.project)?;
        let cache = self.cache;
        let tags = self.tags.get(&pin.project).expect("populated above");
        tags.iter()
            .filter(|tag| version_cmp(tag.as_str(), pin.tag.as_str()) == Ordering::Less)
            .find(|tag| {
                cache
                    .get(&pin.project, tag)
                    .is_ok_and(|snapshot| snapshot_answers(&snapshot, query))
            })
            .cloned()
    }

    fn ensure_tags(&mut self, project: &ProjectName) -> Option<()> {
        if !self.tags.contains_key(project) {
            let mut tags = self.store.list_tags(project).ok()?;
            // `version_cmp` is descending; reverse for oldest-first so
            // the first hit is the earliest tag.
            tags.sort_by(|a, b| version_cmp(a.as_str(), b.as_str()).reverse());
            self.tags.insert(project.clone(), tags);
        }
        Some(())
    }
}

fn snapshot_answers(snapshot: &Snapshot<state::Canonical>, query: &AvailabilityQuery) -> bool {
    match query {
        AvailabilityQuery::ExactArity {
            module,
            function,
            arity,
        } => snapshot.module_named(module).is_some_and(|m| {
            let target = FunArity {
                name: function.clone(),
                arity: *arity,
            };
            m.exports.binary_search(&target).is_ok()
        }),
        AvailabilityQuery::AnyArity { module, function } => snapshot
            .module_named(module)
            .is_some_and(|m| m.exports.iter().any(|fa| &fa.name == function)),
        AvailabilityQuery::Record { name } => snapshot
            .headers()
            .iter()
            .any(|h| h.records.iter().any(|r| &r.name == name)),
    }
}
