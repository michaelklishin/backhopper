// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Sealed marker traits used by the type-state builders.
//!
//! Each builder threads two markers: one for "target slot filled"
//! and one for "input slot filled". `run()` is callable only when
//! both are `With*`, so attempting to dispatch an under-specified
//! verb is a compile-time error.

mod sealed {
    pub trait Sealed {}
}

/// Either [`NoTarget`] or [`WithTarget`]. Sealed.
pub trait TargetState: sealed::Sealed {}
/// Either [`NoInput`] or [`WithInput`]. Sealed.
pub trait InputState: sealed::Sealed {}

/// Marker: target slot is empty.
#[derive(Debug, Clone, Copy)]
pub struct NoTarget;
/// Marker: target slot is filled.
#[derive(Debug, Clone, Copy)]
pub struct WithTarget;

/// Marker: input slot is empty.
#[derive(Debug, Clone, Copy)]
pub struct NoInput;
/// Marker: input slot is filled.
#[derive(Debug, Clone, Copy)]
pub struct WithInput;

impl sealed::Sealed for NoTarget {}
impl sealed::Sealed for WithTarget {}
impl sealed::Sealed for NoInput {}
impl sealed::Sealed for WithInput {}

impl TargetState for NoTarget {}
impl TargetState for WithTarget {}
impl InputState for NoInput {}
impl InputState for WithInput {}
