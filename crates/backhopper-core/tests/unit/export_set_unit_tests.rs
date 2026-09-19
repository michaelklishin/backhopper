// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_exports`: the `Surface` the qualified-call resolver looks
//! keys up against, and the ground that makes a lookup unknowable
//! rather than a plain `bool`.

use std::str::FromStr;

use backhopper_core::compat::source_attributes::{Presence, Unreadable, extract_exports};
use backhopper_core::model::names::{Arity, FunctionName};

fn fa(name: &str, arity: u8) -> (FunctionName, Arity) {
    (FunctionName::from_str(name).unwrap(), Arity::new(arity))
}

#[test]
fn a_plain_export_list_is_known() {
    let surface = extract_exports("-module(m).\n-export([f/0, g/1]).\n");
    assert!(surface.is_complete());
    assert_eq!(surface.lookup(&fa("f", 0)), Presence::Present);
    assert_eq!(surface.lookup(&fa("g", 1)), Presence::Present);
    assert_eq!(surface.lookup(&fa("h", 0)), Presence::Absent);
}

#[test]
fn several_export_attributes_union() {
    let surface = extract_exports("-export([f/0]).\n-export([g/1]).\n");
    assert_eq!(surface.lookup(&fa("f", 0)), Presence::Present);
    assert_eq!(surface.lookup(&fa("g", 1)), Presence::Present);
}

#[test]
fn export_all_makes_the_surface_unknowable() {
    let surface = extract_exports("-compile(export_all).\n");
    assert!(!surface.is_complete());
    assert_eq!(
        surface.lookup(&fa("f", 0)),
        Presence::Unknowable(Unreadable::ExportAll)
    );
    let surface = extract_exports("-compile([export_all, nowarn_export_all]).\n");
    assert!(!surface.is_complete());
}

#[test]
fn a_parse_transform_makes_the_surface_unknowable() {
    let surface = extract_exports("-compile({parse_transform, lager_transform}).\n");
    assert!(!surface.is_complete());
    assert_eq!(
        surface.lookup(&fa("f", 0)),
        Presence::Unknowable(Unreadable::ParseTransform)
    );
}

#[test]
fn a_macro_in_the_export_list_names_itself_as_the_ground() {
    let surface = extract_exports("-export([f/0, ?API]).\n");
    assert!(!surface.is_complete());
    // the readable entry still resolves as present
    assert_eq!(surface.lookup(&fa("f", 0)), Presence::Present);
    assert_eq!(
        surface.lookup(&fa("g", 0)),
        Presence::Unknowable(Unreadable::MacroInExportList)
    );
}

#[test]
fn no_export_attribute_is_known_and_empty() {
    let surface = extract_exports("-module(m).\nf() -> ok.\n");
    assert!(surface.is_complete());
    assert_eq!(surface.lookup(&fa("f", 0)), Presence::Absent);
}

// -export_type entries are types, not functions: they must not enter the function export surface.
#[test]
fn export_type_is_not_a_function_export() {
    let surface = extract_exports("-export_type([t/0]).\n-export([f/1]).\n");
    assert!(surface.is_complete());
    assert_eq!(surface.lookup(&fa("f", 1)), Presence::Present);
    assert_eq!(surface.lookup(&fa("t", 0)), Presence::Absent);
}

// A cross-file add merges surfaces per module: the unreadable side's
// ground carries over, and a listed key stays present on either side.
#[test]
fn merging_an_unreadable_surface_makes_the_union_unreadable() {
    let mut surface = extract_exports("-export([f/0]).\n");
    surface.merge(extract_exports("-compile(export_all).\n-export([g/1]).\n"));
    assert!(!surface.is_complete());
    assert_eq!(surface.lookup(&fa("f", 0)), Presence::Present);
    assert_eq!(surface.lookup(&fa("g", 1)), Presence::Present);
    assert_eq!(
        surface.lookup(&fa("h", 2)),
        Presence::Unknowable(Unreadable::ExportAll)
    );
}

#[test]
fn merging_two_readable_surfaces_stays_readable() {
    let mut surface = extract_exports("-export([f/0]).\n");
    surface.merge(extract_exports("-export([g/1]).\n"));
    assert!(surface.is_complete());
    assert_eq!(surface.lookup(&fa("g", 1)), Presence::Present);
    assert_eq!(surface.lookup(&fa("h", 2)), Presence::Absent);
}

#[test]
fn merging_two_unreadable_surfaces_keeps_the_ground_already_held() {
    let mut surface = extract_exports("-export([f/0, ?API]).\n");
    surface.merge(extract_exports("-compile(export_all).\n"));
    assert_eq!(
        surface.lookup(&fa("h", 2)),
        Presence::Unknowable(Unreadable::MacroInExportList)
    );
}

// A parse transform can rewrite the export list itself, so it outranks
// a ground under which a listed entry still counts as exported.
#[test]
fn a_parse_transform_outranks_export_all_in_either_merge_order() {
    let mut surface = extract_exports("-compile(export_all).\n-export([f/0]).\n");
    surface.merge(extract_exports(
        "-compile({parse_transform, lager_transform}).\n",
    ));
    assert_eq!(
        surface.lookup(&fa("f", 0)),
        Presence::Unknowable(Unreadable::ParseTransform)
    );

    let mut surface = extract_exports("-compile({parse_transform, lager_transform}).\n");
    surface.merge(extract_exports("-compile(export_all).\n-export([f/0]).\n"));
    assert_eq!(
        surface.lookup(&fa("f", 0)),
        Presence::Unknowable(Unreadable::ParseTransform)
    );
}

#[test]
fn a_parse_transform_outranks_export_all_within_one_module() {
    let surface = extract_exports(
        "-compile(export_all).\n-compile({parse_transform, lager_transform}).\n-export([f/0]).\n",
    );
    assert_eq!(
        surface.lookup(&fa("f", 0)),
        Presence::Unknowable(Unreadable::ParseTransform)
    );
}
