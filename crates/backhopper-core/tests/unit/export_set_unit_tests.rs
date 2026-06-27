// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_exports`: the export set the qualified-call resolver checks
//! against, and the `complete` flag that withholds when the surface is
//! not fully knowable from source.

use std::str::FromStr;

use backhopper_core::compat::source_attributes::extract_exports;
use backhopper_core::model::names::{Arity, FunctionName};

fn fa(name: &str, arity: u8) -> (FunctionName, Arity) {
    (FunctionName::from_str(name).unwrap(), Arity::new(arity))
}

#[test]
fn a_plain_export_list_is_complete() {
    let set = extract_exports("-module(m).\n-export([f/0, g/1]).\n");
    assert!(set.complete);
    assert!(set.exports.contains(&fa("f", 0)));
    assert!(set.exports.contains(&fa("g", 1)));
}

#[test]
fn several_export_attributes_union() {
    let set = extract_exports("-export([f/0]).\n-export([g/1]).\n");
    assert!(set.exports.contains(&fa("f", 0)));
    assert!(set.exports.contains(&fa("g", 1)));
}

#[test]
fn export_all_marks_incomplete() {
    assert!(!extract_exports("-compile(export_all).\n").complete);
    assert!(!extract_exports("-compile([export_all, nowarn_export_all]).\n").complete);
}

#[test]
fn parse_transform_marks_incomplete() {
    assert!(!extract_exports("-compile({parse_transform, lager_transform}).\n").complete);
}

#[test]
fn a_macro_in_the_export_list_marks_incomplete() {
    let set = extract_exports("-export([f/0, ?API]).\n");
    assert!(!set.complete);
    // the readable entry is still recorded
    assert!(set.exports.contains(&fa("f", 0)));
}

#[test]
fn no_export_attribute_is_complete_and_empty() {
    let set = extract_exports("-module(m).\nf() -> ok.\n");
    assert!(set.complete);
    assert!(set.exports.is_empty());
}

// `-export_type` is a different attribute: its entries are types, not
// functions, and must not enter the function export set.
#[test]
fn export_type_is_not_a_function_export() {
    let set = extract_exports("-export_type([t/0]).\n-export([f/1]).\n");
    assert!(set.complete);
    assert!(set.exports.contains(&fa("f", 1)));
    assert!(!set.exports.contains(&fa("t", 0)));
}
