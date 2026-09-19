// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! "Where" a check runs: against a series, or against a single pin.
//!
//! `PinSelector` lives in `backhopper_core::model::pin`, shared with the CLI's
//! own selector parsing; this module re-exports it so driver consumers keep
//! their existing import path.

pub use backhopper_core::model::pin::PinSelector;
