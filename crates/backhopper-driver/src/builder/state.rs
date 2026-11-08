// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Sealed marker traits used by the type-state builders.
//!
//! Each builder threads two markers: one for "target slot filled"
//! and one for "input slot filled". `run()` is callable only when
//! both are `With*`, so attempting to dispatch an under-specified
//! verb is a compile-time error.

use std::path::PathBuf;

use backhopper_core::model::names::HostName;
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::HostSeries;

use crate::builder::jigsaw::CheckTarget;

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

/// Either [`NoHost`] or [`WithHost`]. Sealed.
pub trait HostState: sealed::Sealed {}
/// Either [`NoSeries`] or [`WithSeries`]. Sealed.
pub trait SeriesState: sealed::Sealed {}
/// Either [`NoPlugins`] or [`WithPlugins`]. Sealed.
pub trait PluginsState: sealed::Sealed {}

/// Marker: the `jigsaw` host slot is empty.
#[derive(Debug, Clone, Copy)]
pub struct NoHost;
/// Marker: the `jigsaw` host slot holds the host to plan for.
#[derive(Debug, Clone)]
pub struct WithHost(pub(crate) HostName);

/// Marker: the `jigsaw` series slot is empty.
#[derive(Debug, Clone, Copy)]
pub struct NoSeries;
/// Marker: the `jigsaw` series slot holds the series to plan for.
#[derive(Debug, Clone, Copy)]
pub struct WithSeries(pub(crate) HostSeries);

/// Marker: `jigsaw check` has no plugin directory yet.
#[derive(Debug, Clone, Copy)]
pub struct NoPlugins;
/// Marker: `jigsaw check` has at least one plugin directory.
#[derive(Debug, Clone)]
pub struct WithPlugins(pub(crate) NonEmpty<PathBuf>);

/// Marker: `jigsaw check` has its plan source. Unlike [`WithTarget`], it
/// holds the source, because the two forms are different arguments.
#[derive(Debug, Clone)]
pub struct WithCheckTarget(pub(crate) CheckTarget);

impl sealed::Sealed for NoHost {}
impl sealed::Sealed for WithHost {}
impl sealed::Sealed for NoSeries {}
impl sealed::Sealed for WithSeries {}
impl sealed::Sealed for NoPlugins {}
impl sealed::Sealed for WithPlugins {}
impl sealed::Sealed for WithCheckTarget {}

impl HostState for NoHost {}
impl HostState for WithHost {}
impl SeriesState for NoSeries {}
impl SeriesState for WithSeries {}
impl PluginsState for NoPlugins {}
impl PluginsState for WithPlugins {}
impl TargetState for WithCheckTarget {}
