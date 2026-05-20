// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

pub mod format;
pub mod parser;
pub mod sort;
pub mod spec_normalize;

pub const SNAPSHOT_SIZE_LIMIT: usize = 256 * 1024 * 1024;
