// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `jigsaw`: checking out-of-tree apps against every patch release of one
//! host series. This module holds the plan, the loaded plan, the plugin
//! set and the evaluation. `Reference`, `Finding` and `Warning` are named
//! through this module: `model::symbol::Reference` is another type.

pub mod anchor;
pub mod check;
mod check_wire;
mod digest;
mod evaluate;
pub mod findings;
mod loaded;
mod plan;
pub mod plugin_set;
pub mod references;
mod releases;
pub mod report;
mod series_plan;
pub mod surface;
mod tags;
mod wire;

pub use crate::snapshot::freshness::StaleReason;
pub use check_wire::{
    JigsawAppRowPayload, JigsawCheckPayload, JigsawFindingPayload, JigsawRangePayload,
    JigsawSitePayload, JigsawTalliesPayload, JigsawUncheckedPayload, JigsawWarningPayload,
    JigsawWithheldPayload, LIMITS,
};
pub use digest::host_facts_digest;
pub use evaluate::evaluate;
pub use loaded::{CommitMismatch, LoadedPlan, SnapshotGap, StalePin, StoreStatus, UnstockedPlan};
pub use plan::{
    DraftInputs, HostRelease, JigsawPlan, PlanError, ReleaseFacts, TagSourceKind, UnsupportedMajor,
    draft_plan,
};
pub use releases::{HasHostTag, SeriesReleases, SeriesReleasesError};
pub use series_plan::{
    DraftedPlan, ListedTags, ResolvedRelease, SeriesRequest, plan_series, plan_series_from_store,
};
pub use tags::{OtpLine, OtpLineError, SeriesTags, select_otp_lines, select_series_tags};
pub use wire::{JigsawPlanPayload, MissingSnapshotPayload, PlanOtpLinePayload, PlanReleasePayload};
