// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared test fixture builders for the backhopper workspace. Consumed
//! as a path-only dev-dependency, so it never ships in a release.

mod config;
mod git;
mod snapshot;

pub use config::{toml_path, write_config};
pub use git::GitRepoFixture;
pub use snapshot::{canonical_snapshot, module, module_with, pin, snapshot_header};
