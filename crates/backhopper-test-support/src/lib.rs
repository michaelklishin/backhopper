// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared test fixture builders for the backhopper workspace. Consumed
//! as a path-only dev-dependency, so it never ships in a release.

mod config;
mod git;
mod jigsaw;
mod jigsaw_plan;
mod snapshot;
mod world;

pub use config::{toml_path, write_config};
pub use git::GitRepoFixture;
pub use jigsaw::HOST_CONFIG_TOML;
pub use jigsaw_plan::{host_config_excluding_otp_apps, loaded_plan, loaded_plan_with_config};
pub use snapshot::{canonical_snapshot, module, module_with, pin, snapshot_header};
pub use world::{
    SnapshotBuilder, app_names, defining, defining_conditional, exports, hrl, including,
    module_name, with_callbacks, with_exported_types, with_record, with_spec,
};
