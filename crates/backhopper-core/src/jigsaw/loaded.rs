// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::iter;
use std::panic;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::digest::DigestWriter;
use crate::errors::StoreError;
use crate::jigsaw::plan::JigsawPlan;
use crate::jigsaw::surface::{PinApps, PlanSurfaces};
use crate::model::names::{ApplicationName, CommitSha, ProjectName, StoreDigest, vocabulary};
use crate::model::pin::Pin;
use crate::model::snapshot::{Snapshot, SnapshotHeader, state};
use crate::snapshot::freshness::{ExpectedExtraction, StaleReason, extraction_freshness};
use crate::snapshot::parser;
use crate::store::{ReadOnly, SnapshotStore};

vocabulary!(
    pub enum SnapshotGap: "snapshot gap" {
        Missing => "missing",
        Stale => "stale",
        CommitMismatch => "commit_mismatch",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalePin {
    pub pin: Pin,
    pub reason: StaleReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMismatch {
    pub pin: Pin,
    pub expected: CommitSha,
    pub found: CommitSha,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnstockedPlan {
    pub missing: Vec<Pin>,
    pub stale: Vec<StalePin>,
    pub commit_mismatch: Vec<CommitMismatch>,
}

impl UnstockedPlan {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.stale.is_empty() && self.commit_mismatch.is_empty()
    }

    /// Why a stale or mismatched pin is not usable as stored.
    #[must_use]
    pub fn detail(&self, pin: &Pin) -> Option<String> {
        let stale = self
            .stale
            .iter()
            .find(|stale| &stale.pin == pin)
            .map(|stale| stale.reason.to_string());
        stale.or_else(|| {
            self.commit_mismatch
                .iter()
                .find(|mismatch| &mismatch.pin == pin)
                .map(|mismatch| format!("expected {}, found {}", mismatch.expected, mismatch.found))
        })
    }

    #[must_use]
    pub fn gaps(&self) -> Vec<(Pin, SnapshotGap)> {
        let missing = self
            .missing
            .iter()
            .map(|pin| (pin.clone(), SnapshotGap::Missing));
        let stale = self
            .stale
            .iter()
            .map(|s| (s.pin.clone(), SnapshotGap::Stale));
        let mismatched = self
            .commit_mismatch
            .iter()
            .map(|m| (m.pin.clone(), SnapshotGap::CommitMismatch));
        let mut gaps: Vec<_> = missing.chain(stale).chain(mismatched).collect();
        gaps.sort_by(|a, b| a.0.cmp(&b.0));
        gaps
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreStatus {
    Stocked,
    Unstocked(UnstockedPlan),
}

enum Gap {
    Missing,
    Stale(StaleReason),
    CommitMismatch {
        expected: CommitSha,
        found: CommitSha,
    },
}

impl UnstockedPlan {
    fn record(&mut self, pin: Pin, gap: Gap) {
        match gap {
            Gap::Missing => self.missing.push(pin),
            Gap::Stale(reason) => self.stale.push(StalePin { pin, reason }),
            Gap::CommitMismatch { expected, found } => self.commit_mismatch.push(CommitMismatch {
                pin,
                expected,
                found,
            }),
        }
    }
}

fn check_header(
    plan: &JigsawPlan,
    pin: &Pin,
    header: &SnapshotHeader,
    expected: Option<ExpectedExtraction>,
) -> Option<Gap> {
    let Some(expected) = expected else {
        return Some(Gap::Missing);
    };
    if let Some(reason) = extraction_freshness(header, &expected) {
        return Some(Gap::Stale(reason));
    }
    if &pin.project != plan.host_project() {
        return None;
    }
    if header.host_facts.as_ref().map(|facts| &facts.digest) != Some(plan.host_facts()) {
        return Some(Gap::Stale(StaleReason::HostFacts));
    }
    let release = plan
        .releases()
        .iter()
        .find(|release| release.tag.tag() == &pin.tag)?;
    (release.commit != header.commit).then(|| Gap::CommitMismatch {
        expected: release.commit.clone(),
        found: header.commit.clone(),
    })
}

fn unreadable(error: &StoreError) -> Gap {
    match error {
        StoreError::SnapshotNotFound { .. } => Gap::Missing,
        _ => Gap::Stale(StaleReason::Unreadable),
    }
}

impl JigsawPlan {
    pub fn store_status(
        &self,
        store: &SnapshotStore<ReadOnly>,
        expected: impl Fn(&ProjectName) -> Option<ExpectedExtraction>,
    ) -> StoreStatus {
        let mut unstocked = UnstockedPlan::default();
        for pin in self.required() {
            let gap = match store.read_header(&pin.project, &pin.tag) {
                Ok(header) => check_header(self, &pin, &header, expected(&pin.project)),
                Err(error) => Some(unreadable(&error)),
            };
            if let Some(gap) = gap {
                unstocked.record(pin, gap);
            }
        }
        if unstocked.is_empty() {
            StoreStatus::Stocked
        } else {
            StoreStatus::Unstocked(unstocked)
        }
    }

    #[must_use]
    pub fn nothing_stored(&self) -> StoreStatus {
        StoreStatus::Unstocked(self.all_missing())
    }

    /// What `load` would report against a store with nothing in it.
    #[must_use]
    pub fn all_missing(&self) -> UnstockedPlan {
        UnstockedPlan {
            missing: self.required().into_iter().collect(),
            ..UnstockedPlan::default()
        }
    }

    pub fn load(
        self,
        store: &SnapshotStore<ReadOnly>,
        expected: impl Fn(&ProjectName) -> Option<ExpectedExtraction> + Sync,
    ) -> Result<LoadedPlan, UnstockedPlan> {
        let pins: Vec<Pin> = self.required().into_iter().collect();
        let outcomes = read_all(&self, store, &expected, &pins);
        let mut unstocked = UnstockedPlan::default();
        let mut snapshots = BTreeMap::new();
        for (pin, outcome) in pins.into_iter().zip(outcomes) {
            match outcome {
                Ok(snapshot) => {
                    snapshots.insert(pin, snapshot);
                }
                Err(gap) => unstocked.record(pin, gap),
            }
        }
        if !unstocked.is_empty() {
            return Err(unstocked);
        }
        let surfaces = Surfaces::build(&self, &snapshots)?;
        let store_digest = store_digest(&snapshots);
        Ok(LoadedPlan {
            plan: self,
            snapshots,
            store_digest,
            surfaces,
        })
    }
}

fn read_one(
    plan: &JigsawPlan,
    store: &SnapshotStore<ReadOnly>,
    expected: Option<ExpectedExtraction>,
    pin: &Pin,
) -> Result<Snapshot<state::Canonical>, Gap> {
    let text = store
        .read_text(&pin.project, &pin.tag)
        .map_err(|e| unreadable(&e))?;
    let header = parser::parse_header(&text).map_err(|_| Gap::Stale(StaleReason::Unreadable))?;
    if let Some(gap) = check_header(plan, pin, &header, expected) {
        return Err(gap);
    }
    parser::parse(&text).map_err(|_| Gap::Stale(StaleReason::Unreadable))
}

/// Reads on scoped threads: the OTP snapshots are most of the bytes.
fn read_all(
    plan: &JigsawPlan,
    store: &SnapshotStore<ReadOnly>,
    expected: &(impl Fn(&ProjectName) -> Option<ExpectedExtraction> + Sync),
    pins: &[Pin],
) -> Vec<Result<Snapshot<state::Canonical>, Gap>> {
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(pins.len().max(1));
    let next = AtomicUsize::new(0);
    let mut outcomes = Vec::with_capacity(pins.len());
    thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(pin) = pins.get(index) else {
                            break done;
                        };
                        done.push((index, read_one(plan, store, expected(&pin.project), pin)));
                    }
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(done) => outcomes.extend(done),
                Err(payload) => panic::resume_unwind(payload),
            }
        }
    });
    outcomes.sort_by_key(|(index, _)| *index);
    outcomes.into_iter().map(|(_, outcome)| outcome).collect()
}

fn store_digest(snapshots: &BTreeMap<Pin, Snapshot<state::Canonical>>) -> StoreDigest {
    let mut writer = DigestWriter::new("jigsaw store");
    writer.count(snapshots.len());
    for (pin, snapshot) in snapshots {
        writer
            .text(pin.project.as_str())
            .text(pin.tag.as_str())
            .text(snapshot.header().commit.as_str())
            .text(&snapshot.header().extractor_version);
    }
    StoreDigest::from_digest(writer.finish())
}

#[derive(Debug)]
struct Surfaces {
    build_app_names: BTreeSet<ApplicationName>,
    pin_apps: BTreeMap<Pin, PinApps>,
}

impl Surfaces {
    fn build(
        plan: &JigsawPlan,
        snapshots: &BTreeMap<Pin, Snapshot<state::Canonical>>,
    ) -> Result<Self, UnstockedPlan> {
        let mut unstocked = UnstockedPlan::default();
        let mut pin_apps = BTreeMap::new();
        for (pin, snapshot) in snapshots {
            match PinApps::of(snapshot) {
                Some(apps) => {
                    pin_apps.insert(pin.clone(), apps);
                }
                None => unstocked.record(pin.clone(), Gap::Stale(StaleReason::MissingApp)),
            }
        }
        if !unstocked.is_empty() {
            return Err(unstocked);
        }
        let build = plan.releases().build();
        let build_pins = iter::once(Pin::new(
            plan.host_project().clone(),
            build.tag.tag().clone(),
        ))
        .chain(
            build
                .pins
                .iter()
                .map(|(project, tag)| Pin::new(project.clone(), tag.clone())),
        )
        .chain(plan.otp_lines().iter().flat_map(|line| {
            line.tags()
                .iter()
                .map(|tag| Pin::new(plan.otp_project().clone(), tag.tag().clone()))
        }));
        let build_app_names = build_pins
            .filter_map(|pin| snapshots.get(&pin))
            .flat_map(|snapshot| snapshot.apps().iter().map(|app| app.name.clone()))
            .collect();
        Ok(Self {
            build_app_names,
            pin_apps,
        })
    }
}

#[derive(Debug)]
pub struct LoadedPlan {
    plan: JigsawPlan,
    snapshots: BTreeMap<Pin, Snapshot<state::Canonical>>,
    store_digest: StoreDigest,
    surfaces: Surfaces,
}

impl LoadedPlan {
    #[must_use]
    pub fn plan(&self) -> &JigsawPlan {
        &self.plan
    }

    #[must_use]
    pub fn snapshot(&self, pin: &Pin) -> Option<&Snapshot<state::Canonical>> {
        self.snapshots.get(pin)
    }

    #[must_use]
    pub fn snapshots(&self) -> &BTreeMap<Pin, Snapshot<state::Canonical>> {
        &self.snapshots
    }

    /// Borrowing views of every release and OTP tag in the plan. They are
    /// built here, on each call, because a view that borrows the snapshots
    /// cannot be stored next to them.
    #[must_use]
    pub fn surfaces(&self) -> PlanSurfaces<'_> {
        PlanSurfaces::new(&self.plan, &self.snapshots, &self.surfaces.pin_apps)
    }

    #[must_use]
    pub fn store_digest(&self) -> &StoreDigest {
        &self.store_digest
    }

    /// Every app the build patch's surface provides: the host snapshot's
    /// apps, the pinned dependency snapshots' apps and the apps of every
    /// OTP tag in the run.
    #[must_use]
    pub fn build_app_names(&self) -> &BTreeSet<ApplicationName> {
        &self.surfaces.build_app_names
    }
}
