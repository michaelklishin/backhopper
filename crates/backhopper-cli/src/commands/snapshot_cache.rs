// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! In-process cache wrapping `SnapshotStore::read`. Shared across the
//! `check batch` loop so the same `(project, tag)` snapshot is parsed
//! from disk at most once per process invocation.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use backhopper_core::errors::StoreError;
use backhopper_core::model::names::{ProjectName, TagName};
use backhopper_core::model::snapshot::{Snapshot, state};
use backhopper_core::store::{ReadOnly, SnapshotStore};

type CacheKey = (ProjectName, TagName);
type CacheValue = Arc<Snapshot<state::Canonical>>;

#[derive(Debug)]
pub struct SnapshotCache<'a> {
    store: &'a SnapshotStore<ReadOnly>,
    entries: Mutex<BTreeMap<CacheKey, CacheValue>>,
}

impl<'a> SnapshotCache<'a> {
    pub fn new(store: &'a SnapshotStore<ReadOnly>) -> Self {
        Self {
            store,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn get(
        &self,
        project: &ProjectName,
        tag: &TagName,
    ) -> Result<Arc<Snapshot<state::Canonical>>, StoreError> {
        let key = (project.clone(), tag.clone());
        {
            let guard = self.entries.lock().expect("snapshot cache mutex poisoned");
            if let Some(arc) = guard.get(&key) {
                return Ok(Arc::clone(arc));
            }
        }
        let snap = self.store.read(project, tag)?;
        let arc = Arc::new(snap);
        let mut guard = self.entries.lock().expect("snapshot cache mutex poisoned");
        Ok(guard.entry(key).or_insert_with(|| Arc::clone(&arc)).clone())
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .expect("snapshot cache mutex poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .expect("snapshot cache mutex poisoned")
            .is_empty()
    }
}
