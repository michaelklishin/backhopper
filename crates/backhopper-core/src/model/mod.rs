// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

pub mod apply;
pub mod batch;
pub mod cache;
pub mod check_payload;
pub mod clearance;
pub mod eval;
pub mod evaluation;
pub mod findings;
pub mod fingerprint;
pub mod names;
pub mod pin;
pub mod pr_commit;
pub mod resolver_coverage;
pub(crate) mod serde_util;
pub mod snapshot;
pub mod snapshot_diff;
pub mod spec_ast;
pub mod spec_parser;
pub mod summary;
pub mod symbol;
pub mod verdict;
pub mod wire_envelope;
