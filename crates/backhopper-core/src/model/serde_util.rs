// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Shared `skip_serializing_if` predicates for the model types.

// serde's `skip_serializing_if` predicate shape requires `&bool`.
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_zero(n: &usize) -> bool {
    *n == 0
}
