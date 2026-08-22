// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `scan_hunk`: joins same-context hunk runs so a call or clause head
//! whose argument list wraps across lines resolves at exact arity, and
//! attributes each reference by the line its match started on.

use backhopper_core::compat::call_sites::scan_hunk;
use backhopper_core::erlang_macros::MacroTable;
use backhopper_core::model::symbol::{RefOrigin, SymbolKind, SymbolRef};

fn added(text: &str) -> (RefOrigin, &str) {
    (RefOrigin::Added, text)
}

fn context(text: &str) -> (RefOrigin, &str) {
    (RefOrigin::Context, text)
}

fn scan(lines: &[(RefOrigin, &str)]) -> backhopper_core::compat::call_sites::HunkScan {
    scan_hunk(lines, &MacroTable::new())
}

// m:f/a and the origin, for each function or any-arity reference.
fn functions(refs: &[SymbolRef]) -> Vec<(String, RefOrigin)> {
    refs.iter()
        .filter_map(|r| match &r.kind {
            SymbolKind::Function { mfa } => Some((mfa.to_string(), r.origin)),
            SymbolKind::FunctionAnyArity { module, function } => {
                Some((format!("{module}:{function}/?"), r.origin))
            }
            _ => None,
        })
        .collect()
}

fn types(refs: &[SymbolRef]) -> Vec<String> {
    refs.iter()
        .filter_map(|r| match &r.kind {
            SymbolKind::Type {
                module,
                name,
                arity,
            } => Some(format!("{module}:{name}/{arity}")),
            _ => None,
        })
        .collect()
}

fn defined(refs: &[SymbolRef]) -> Vec<String> {
    refs.iter()
        .filter_map(|r| match &r.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            SymbolKind::FunctionAnyArity { module, function } => {
                Some(format!("{module}:{function}/?"))
            }
            _ => None,
        })
        .collect()
}

// A call whose argument list closes on a later line resolves at exact arity, not FunctionAnyArity.
#[test]
fn a_wrapped_call_resolves_to_exact_arity() {
    let scan = scan(&[
        added("    ok = rabbit_misc:queue_resource(V,"),
        added("        Q)"),
    ]);
    assert_eq!(
        functions(&scan.referenced),
        [("rabbit_misc:queue_resource/2".to_owned(), RefOrigin::Added)]
    );
}

#[test]
fn a_single_line_call_is_unchanged() {
    let scan = scan(&[added("    ok = rabbit_misc:queue_resource(V, Q).")]);
    assert_eq!(
        functions(&scan.referenced),
        [("rabbit_misc:queue_resource/2".to_owned(), RefOrigin::Added)]
    );
}

// A call whose head is on a Context line stays Context even when a later argument line is Added.
#[test]
fn the_calls_origin_is_its_head_line() {
    let scan = scan(&[
        context("    ok = ra_log:append(V,"),
        added("        NewArg)"),
    ]);
    assert_eq!(
        functions(&scan.referenced),
        [("ra_log:append/2".to_owned(), RefOrigin::Context)]
    );
}

#[test]
fn an_added_head_drives_the_reference() {
    let scan = scan(&[added("    ok = ra_log:append(V,"), context("        Arg)")]);
    assert_eq!(
        functions(&scan.referenced),
        [("ra_log:append/2".to_owned(), RefOrigin::Added)]
    );
}

// A construct starting on a later line of a run takes its own start line's origin, not the run's first.
#[test]
fn each_construct_takes_its_own_line_origin() {
    let scan = scan(&[context("    aten:register(X),"), added("    khepri:get(Y)")]);
    let mut got = functions(&scan.referenced);
    got.sort();
    assert_eq!(
        got,
        [
            ("aten:register/1".to_owned(), RefOrigin::Context),
            ("khepri:get/1".to_owned(), RefOrigin::Added),
        ]
    );
}

#[test]
fn a_wrapped_clause_head_recovers_its_arity() {
    let scan = scan(&[
        added("handle_call(Request,"),
        added("            From, State) -> ok."),
    ]);
    assert_eq!(defined(&scan.defined), ["_local:handle_call/3".to_owned()]);
}

// A fun M:F/A split across lines: the per-line scanner matched neither line; the join scans it whole.
#[test]
fn a_wrapped_fun_ref_resolves() {
    let scan = scan(&[
        added("    F = fun rabbit_misc:"),
        added("            queue_resource/2,"),
    ]);
    assert_eq!(
        functions(&scan.referenced),
        [("rabbit_misc:queue_resource/2".to_owned(), RefOrigin::Added)]
    );
}

// A -spec run and the following body scan as separate regions: a type reference, then a call.
#[test]
fn the_spec_region_does_not_merge_into_the_body() {
    let scan = scan(&[
        added("-spec append(ra:index()) -> ok."),
        added("append(X) -> ra_lib:id(X)."),
    ]);
    assert_eq!(
        functions(&scan.referenced),
        [("ra_lib:id/1".to_owned(), RefOrigin::Added)]
    );
    assert_eq!(defined(&scan.defined), ["_local:append/1".to_owned()]);
    let has_type = scan
        .referenced
        .iter()
        .any(|r| matches!(&r.kind, SymbolKind::Type { name, .. } if name.as_str() == "index"));
    assert!(
        has_type,
        "expected ra:index/0 type ref: {:?}",
        scan.referenced
    );
}

// A type reference wrapping inside the spec region closes across the join at exact arity.
#[test]
fn a_wrapped_type_reference_resolves() {
    let scan = scan(&[
        added("-spec lookup(dict:dict(K,"),
        added("            V)) -> ok."),
    ]);
    let arity = scan.referenced.iter().find_map(|r| match &r.kind {
        SymbolKind::Type {
            module,
            name,
            arity,
        } if module.as_str() == "dict" && name.as_str() == "dict" => Some(arity.get()),
        _ => None,
    });
    assert_eq!(arity, Some(2));
}

// Only Added-head calls feed the clause-mismatch comparison.
#[test]
fn call_args_keep_only_added_head_calls() {
    let scan = scan(&[added("    aten:register(X),"), context("    khepri:get(Y)")]);
    let names: Vec<String> = scan
        .call_args
        .iter()
        .map(|(mfa, _)| mfa.to_string())
        .collect();
    assert_eq!(names, ["aten:register/1".to_owned()]);
}

// Several clause heads in one body run: the multi-line anchor finds a head at every physical line start.
#[test]
fn multiple_clause_heads_in_one_run_are_all_defined() {
    let scan = scan(&[
        added("apply(leader) -> 1;"),
        added("apply(follower, candidate) -> 2."),
    ]);
    let mut defs = defined(&scan.defined);
    defs.sort();
    assert_eq!(
        defs,
        ["_local:apply/1".to_owned(), "_local:apply/2".to_owned()]
    );
}

#[test]
fn an_empty_hunk_scans_to_nothing() {
    let scan = scan(&[]);
    assert!(scan.referenced.is_empty());
    assert!(scan.defined.is_empty());
}

// The HF-50 shape: a wrapped lists:foreach whose first argument is an
// inline fun carrying statement commas must read /2, not /4.
#[test]
fn a_wrapped_call_with_an_inline_fun_argument_scans_at_exact_arity() {
    let scan = scan(&[
        added("    lists:foreach("),
        added("      fun({Name, Val}) ->"),
        added("              C = connect(atom_to_binary(Name), Config),"),
        added("              ok = emqtt:publish(C, Topic, #{Name => Val},"),
        added("                                 atom_to_binary(Name), [{qos, 0}]),"),
        added("              util:await_exit(C)"),
        added("      end, NotApplicable),"),
    ]);
    let foreach: Vec<_> = functions(&scan.referenced)
        .into_iter()
        .filter(|(m, _)| m.starts_with("lists:foreach"))
        .collect();
    assert_eq!(foreach, [("lists:foreach/2".to_owned(), RefOrigin::Added)]);
}

// documentation prose is string content: it contributes no references
// and does not join the lines around it into one construct
#[test]
fn a_documentation_block_contributes_nothing_and_breaks_the_run() {
    let out = scan(&[
        added("-moduledoc \"\"\""),
        added("Applies commands. See ra_machine:apply(1, 2, 3)."),
        added("\"\"\"."),
        added("tick() -> ra_server:tick(1)."),
    ]);
    assert_eq!(
        functions(&out.referenced),
        vec![("ra_server:tick/1".to_string(), RefOrigin::Added)]
    );
}

#[test]
fn a_call_split_across_a_documentation_block_does_not_join_into_one() {
    let out = scan(&[
        added("tick() -> ra_server:tick("),
        added("-doc \"\"\""),
        added("prose"),
        added("\"\"\"."),
        added("1)."),
    ]);
    let names: Vec<_> = functions(&out.referenced)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(names, vec!["ra_server:tick/?".to_string()]);
}

// HF-45's record addendum: a field's `::` annotation is a type
// reference, not a call into the same name.
#[test]
fn a_record_field_type_lands_as_a_type_reference() {
    let out = scan(&[
        added("-record(state, {"),
        added("    vhost :: rabbit_types:vhost()"),
        added("})."),
    ]);
    assert_eq!(types(&out.referenced), ["rabbit_types:vhost/0".to_owned()]);
    assert!(
        functions(&out.referenced).is_empty(),
        "unexpected: {:?}",
        out.referenced
    );
}

// The same field's default expression still calls a function, and its
// annotation is still a type reference: both halves route correctly.
#[test]
fn a_record_field_default_call_lands_as_a_function_reference() {
    let out = scan(&[
        added("-record(state, {"),
        added("    timeout = rabbit_misc:get_timeout() :: rabbit_types:vhost()"),
        added("})."),
    ]);
    assert_eq!(
        functions(&out.referenced),
        [("rabbit_misc:get_timeout/0".to_owned(), RefOrigin::Added)]
    );
    assert_eq!(types(&out.referenced), ["rabbit_types:vhost/0".to_owned()]);
}
