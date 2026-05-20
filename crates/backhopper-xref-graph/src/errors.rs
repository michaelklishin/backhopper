// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use thiserror::Error;

/// Errors the graph crate's public API may return.
///
/// Today the public surface is total: every operation always succeeds. This
/// type exists for forward compatibility: adding a variant later is a minor
/// version bump rather than a breaking one.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GraphError {}