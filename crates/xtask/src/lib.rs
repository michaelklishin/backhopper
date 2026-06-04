// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Thin re-exports from `backhopper_core::schema` so the `xtask`
//! binary and its drift-check tests stay in lockstep with the rest
//! of the workspace.

pub use backhopper_core::schema::{
    CURRENT_SCHEMA_VERSION, MIN_EMBEDDED_VERSION, SchemaError, embedded_versions, rendered_schema,
    schema_value_for,
};
