// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! End-to-end checks that clone live upstream RabbitMQ-ecosystem repos
//! into a temp directory, run `discover` against a couple of recent tags,
//! and verify the resulting snapshots contain expected exports.
//!
//! Every test in this binary is `#[ignore]`d so the default offline
//! profile skips them. Run via:
//!
//!     cargo nextest run --profile online --run-ignored only

mod clone_and_discover_online_tests;
