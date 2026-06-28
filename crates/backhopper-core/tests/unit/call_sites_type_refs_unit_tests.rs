// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::{AttrCtxScanner, extract_into, extract_type_refs_into};
use backhopper_core::model::symbol::RefContext;

fn type_refs(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    extract_type_refs_into(line, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Type {
                module,
                name,
                arity,
            } => Some(format!("{module}:{name}/{arity}")),
            _ => None,
        })
        .collect()
}

fn body_mfas(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    extract_into(line, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn type_extractor_captures_zero_arity_remote_type() {
    let refs = type_refs("-spec compute([ra:index()]) -> ok.");
    assert!(refs.iter().any(|t| t == "ra:index/0"), "got: {refs:?}");
}

#[test]
fn type_extractor_captures_remote_type_with_args() {
    let refs = type_refs("-spec call(ets:tab(), pos_integer()) -> term().");
    assert!(refs.iter().any(|t| t == "ets:tab/0"), "got: {refs:?}");
}

#[test]
fn type_extractor_does_not_emit_function_refs() {
    let mut out = Vec::new();
    extract_type_refs_into("-spec fetch([ra:index()]) -> ok.", &mut out);
    assert!(
        out.iter()
            .all(|s| !matches!(s.kind, SymbolKind::Function { .. })),
        "type extractor should not produce Function refs, got: {out:?}"
    );
}

#[test]
fn type_extractor_captures_macro_uses_in_spec() {
    let mut out = Vec::new();
    extract_type_refs_into("-spec allocate(?MAX_SIZE) -> ok.", &mut out);
    let macros: Vec<_> = out
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Macro { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(macros.iter().any(|m| m == "MAX_SIZE"), "macros: {macros:?}");
}

#[test]
fn body_extractor_still_emits_function_refs_for_calls() {
    let mfas = body_mfas("    ra:index() + 1");
    assert!(mfas.iter().any(|m| m == "ra:index/0"), "got: {mfas:?}");
}

#[test]
fn scanner_classifies_body_line_as_body() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(s.classify("increment(X) -> X + 1."), RefContext::Body);
}

#[test]
fn scanner_classifies_single_line_spec_as_type_attribute() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(
        s.classify("-spec handle(int()) -> ok."),
        RefContext::TypeAttribute
    );
    // After a self-closing spec, the next line is body again.
    assert_eq!(s.classify("handle(X) -> X."), RefContext::Body);
}

#[test]
fn scanner_classifies_multiline_spec_through_to_closing_period() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(s.classify("-spec merge(int(),"), RefContext::TypeAttribute);
    assert_eq!(
        s.classify("           atom()) ->"),
        RefContext::TypeAttribute
    );
    assert_eq!(s.classify("    ok."), RefContext::TypeAttribute);
    assert_eq!(s.classify("merge(X, Y) -> X."), RefContext::Body);
}

#[test]
fn scanner_classifies_callback_type_and_opaque_as_type_attribute() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(
        s.classify("-callback handle_call(term(), term(), term()) -> ok."),
        RefContext::TypeAttribute
    );
    assert_eq!(
        s.classify("-type state() :: #state{}."),
        RefContext::TypeAttribute
    );
    assert_eq!(
        s.classify("-opaque token() :: binary()."),
        RefContext::TypeAttribute
    );
}

#[test]
fn scanner_treats_export_attribute_as_body() {
    let mut s = AttrCtxScanner::new();
    // -export is not a type-context attribute.
    assert_eq!(s.classify("-export([start/1, stop/2])."), RefContext::Body);
}

#[test]
fn scanner_treats_minus_prefixed_function_names_as_body() {
    let mut s = AttrCtxScanner::new();
    // A function named `spectacular` should not look like `-spec`.
    assert_eq!(s.classify("spectacular(X) -> X."), RefContext::Body);
}

#[test]
fn scanner_trailing_comment_does_not_block_closing_period() {
    let mut s = AttrCtxScanner::new();
    assert_eq!(
        s.classify("-spec flush() -> ok.  %% trailing"),
        RefContext::TypeAttribute
    );
    // Period was outside the comment, so we are back to body.
    assert_eq!(s.classify("flush() -> ok."), RefContext::Body);
}
