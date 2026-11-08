// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Reads a plugin set into the `ReferenceSet` that `jigsaw check` compares
//! with the build patch and each target. The reading uses the call reader
//! and the Cuttlefish parser, which depend on `backhopper-core`, so it
//! cannot live there.

mod collect;
mod includes;
mod schema;
mod source;
mod synthetic;
mod uses;

pub use collect::{CollectError, collect};
