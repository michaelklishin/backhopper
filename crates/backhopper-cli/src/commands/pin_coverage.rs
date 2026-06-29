// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared pin-coverage classification. `doctor` and `snapshots verify`
//! both resolve a `PinSpec` against the store and check on-disk
//! presence, so the resolve-and-classify decision lives here once and
//! the two commands cannot disagree.

use backhopper_core::model::names::ProjectName;
use backhopper_core::model::pin::{Pin, PinSpec};
use backhopper_core::store::SnapshotStore;

/// How one `PinSpec` lands against the snapshot store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCoverage {
    /// The spec resolved to a concrete pin; `present` is whether the
    /// store holds its snapshot on disk.
    Resolved { pin: Pin, present: bool },
    /// A `Pattern` spec that matched no stored tag.
    Unresolved { project: ProjectName },
    /// A `SelfRef` spec: it resolves against a working repo, not the
    /// store, so a store-only check cannot place it.
    SelfPin,
}

/// Resolve `spec` against `store` and report its coverage. `SelfRef`
/// pins short-circuit: they need the working repo this layer does not
/// see.
pub fn classify_pin<M>(spec: &PinSpec, store: &SnapshotStore<M>) -> PinCoverage {
    if spec.is_self() {
        return PinCoverage::SelfPin;
    }
    match spec.resolve(store) {
        Ok(pin) => {
            let present = store.has(&pin.project, &pin.tag);
            PinCoverage::Resolved { pin, present }
        }
        Err(_) => PinCoverage::Unresolved {
            project: spec.project().clone(),
        },
    }
}
