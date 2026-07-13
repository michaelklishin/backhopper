// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_indirect_calls`: `m:f/a` references carried as arguments
//! to meck expectations and rpc forms, and their resolution against
//! the target tree through `analyse_qualified_calls`.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::indirect_calls::{IndirectExtraction, extract_indirect_calls};
use backhopper_core::compat::qualified_call_resolve::{
    PatchProvided, QualifiedCallAnalysis, analyse_qualified_calls, patch_provided,
};
use backhopper_core::model::names::{Arity, FunctionName, ModuleName, RelativePath};
use backhopper_core::model::verdict::{IndirectCallForm, Reason};

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn extract(src: &str) -> IndirectExtraction {
    let line_map: Vec<u32> = (1..=src.lines().count() as u32).collect();
    extract_indirect_calls(src, &line_map)
}

fn sites(src: &str) -> Vec<(String, IndirectCallForm, u32)> {
    extract(src)
        .sites
        .into_iter()
        .map(|s| (s.mfa.to_string(), s.via, s.line))
        .collect()
}

// Extraction: each table form with real names.

#[test]
fn meck_expect_with_integer_arity_extracts() {
    let src = "setup() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_queue_type:drain/1".to_owned(),
            IndirectCallForm::MeckExpect,
            1
        )]
    );
}

#[test]
fn meck_expect_with_fun_heads_of_various_arities_extracts() {
    let src = "s() ->\n\
               meck:expect(rabbit_khepri, is_enabled, fun() -> true end),\n\
               meck:expect(rabbit_queue_type, drain, fun(_) -> ok end),\n\
               meck:expect(rabbit_misc, table_lookup, fun(T, K, D) -> {T, K, D} end).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_khepri:is_enabled/0".to_owned(),
                IndirectCallForm::MeckExpect,
                2
            ),
            (
                "rabbit_queue_type:drain/1".to_owned(),
                IndirectCallForm::MeckExpect,
                3
            ),
            (
                "rabbit_misc:table_lookup/3".to_owned(),
                IndirectCallForm::MeckExpect,
                4
            ),
        ]
    );
}

#[test]
fn meck_expect_with_fun_captures_extracts() {
    let src = "s() ->\n\
               meck:expect(rabbit_queue_type, drain, fun rabbit_misc:const/1),\n\
               meck:expect(rabbit_queue_type, revive, fun local_revive/0).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_queue_type:drain/1".to_owned(),
                IndirectCallForm::MeckExpect,
                2
            ),
            (
                "rabbit_queue_type:revive/0".to_owned(),
                IndirectCallForm::MeckExpect,
                3
            ),
        ]
    );
}

#[test]
fn rpc_and_erpc_call_forms_extract() {
    let src = "f(Node, Q) ->\n\
               rpc:call(Node, rabbit_maintenance, drain, []),\n\
               rpc:call(Node, rabbit_amqqueue, lookup, [Q], 5000),\n\
               erpc:call(Node, rabbit_maintenance, revive, []),\n\
               erpc:call(Node, rabbit_amqqueue, delete, [Q, false, false], 5000).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_maintenance:drain/0".to_owned(),
                IndirectCallForm::RpcCall,
                2
            ),
            (
                "rabbit_amqqueue:lookup/1".to_owned(),
                IndirectCallForm::RpcCall,
                3
            ),
            (
                "rabbit_maintenance:revive/0".to_owned(),
                IndirectCallForm::ErpcCall,
                4
            ),
            (
                "rabbit_amqqueue:delete/3".to_owned(),
                IndirectCallForm::ErpcCall,
                5
            ),
        ]
    );
}

#[test]
fn rabbit_misc_rpc_call_forms_extract() {
    let src = "f(Node, Q) ->\n\
               rabbit_misc:rpc_call(Node, rabbit_plugins, list, []),\n\
               rabbit_misc:rpc_call(Node, rabbit_amqqueue, lookup, [Q], 5000).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_plugins:list/0".to_owned(),
                IndirectCallForm::RabbitMiscRpcCall,
                2
            ),
            (
                "rabbit_amqqueue:lookup/1".to_owned(),
                IndirectCallForm::RabbitMiscRpcCall,
                3
            ),
        ]
    );
}

#[test]
fn ct_broker_helper_rpc_overloads_extract_at_their_positions() {
    let src = "f(Config, Node) ->\n\
               rabbit_ct_broker_helpers:rpc(Config, rabbit_maintenance, status, []),\n\
               rabbit_ct_broker_helpers:rpc(Config, Node, rabbit_maintenance, drain, []),\n\
               rabbit_ct_broker_helpers:rpc(Config, 0, rabbit_maintenance, revive, [], 5000),\n\
               rabbit_ct_broker_helpers:rpc_all(Config, rabbit_khepri, is_enabled, []),\n\
               rabbit_ct_broker_helpers:rpc_all(Config, rabbit_khepri, is_enabled, [], 5000).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_maintenance:status/0".to_owned(),
                IndirectCallForm::CtBrokerHelperRpc,
                2
            ),
            (
                "rabbit_maintenance:drain/0".to_owned(),
                IndirectCallForm::CtBrokerHelperRpc,
                3
            ),
            (
                "rabbit_maintenance:revive/0".to_owned(),
                IndirectCallForm::CtBrokerHelperRpc,
                4
            ),
            (
                "rabbit_khepri:is_enabled/0".to_owned(),
                IndirectCallForm::CtBrokerHelperRpc,
                5
            ),
            (
                "rabbit_khepri:is_enabled/0".to_owned(),
                IndirectCallForm::CtBrokerHelperRpc,
                6
            ),
        ]
    );
}

// The motivating incident's exact shape: a meck expectation carried
// through the CT broker rpc helper unwraps to the mecked MFA.
#[test]
fn a_meck_expectation_through_the_rpc_helper_unwraps() {
    let src = "t(Config, Node) ->\n\
               rabbit_ct_broker_helpers:rpc(Config, Node, meck, expect,\n\
                   [rabbit_queue_type, drain, 1,\n\
                    meck:raise(exit, injected_drain_failure)]),\n\
               rabbit_ct_broker_helpers:rpc(Config, Node, meck, expect,\n\
                   [rabbit_queue_type, revive, 0,\n\
                    meck:raise(exit, injected_revive_failure)]).\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_queue_type:drain/1".to_owned(),
                IndirectCallForm::MeckExpect,
                2
            ),
            (
                "rabbit_queue_type:revive/0".to_owned(),
                IndirectCallForm::MeckExpect,
                5
            ),
        ]
    );
}

#[test]
fn an_rpcd_meck_expect_with_a_fun_unwraps_too() {
    let src =
        "t(N) -> rpc:call(N, meck, expect, [rabbit_khepri, is_enabled, fun() -> false end]).\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_khepri:is_enabled/0".to_owned(),
            IndirectCallForm::MeckExpect,
            1
        )]
    );
}

#[test]
fn quoted_atoms_extract() {
    let src = "s() -> meck:expect('rabbit_queue_type', 'drain', 1, ok).\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_queue_type:drain/1".to_owned(),
            IndirectCallForm::MeckExpect,
            1
        )]
    );
}

#[test]
fn a_wrapped_multi_line_form_recovers_exact_arity() {
    let src = "t(Config, Node) ->\n\
               rabbit_ct_broker_helpers:rpc(Config, Node,\n\
                                            rabbit_amqqueue,\n\
                                            delete,\n\
                                            [Q, false,\n\
                                             false]).\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_amqqueue:delete/3".to_owned(),
            IndirectCallForm::CtBrokerHelperRpc,
            2
        )]
    );
}

// Non-adjacent file lines never concatenate: the same construct split
// across two hunks is not scanned as one.
#[test]
fn fragments_from_different_hunks_do_not_concatenate() {
    let src = "t(Config, Node) ->\n\
               rabbit_ct_broker_helpers:rpc(Config, Node, meck, expect,\n\
               [rabbit_queue_type, drain, 1, ok]).\n";
    let gapped = [1u32, 2, 40];
    let out = extract_indirect_calls(src, &gapped);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

// Withholding and skipping.

#[test]
fn a_variable_module_or_function_skips_without_counting() {
    let src = "f(Node, Mod, Fun) ->\n\
               rpc:call(Node, Mod, drain, []),\n\
               rpc:call(Node, rabbit_maintenance, Fun, []).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

#[test]
fn a_variable_argument_list_counts_as_withheld() {
    let src = "f(Node, Args) -> rpc:call(Node, rabbit_maintenance, drain, Args).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn a_cons_tail_counts_as_withheld() {
    let src = "f(Node, Rest) -> rpc:call(Node, rabbit_amqqueue, lookup, [q | Rest]).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

// An atom starting with `fun` is not a fun expression.
#[test]
fn a_fun_prefixed_atom_is_not_read_as_a_fun() {
    let src = "s() -> meck:expect(rabbit_queue_type, drain, funny(1)).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn a_non_literal_meck_arity_counts_as_withheld() {
    let src = "s(N) -> meck:expect(rabbit_queue_type, drain, N, ok).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn an_appended_list_is_not_read_as_a_literal() {
    let src = "f(Node, More) -> rpc:call(Node, rabbit_amqqueue, lookup, [q] ++ More).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn an_unreadable_rpcd_meck_argument_list_counts_as_withheld() {
    let src = "t(N, Args) -> rpc:call(N, meck, expect, [rabbit_queue_type, drain]).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn nested_commas_inside_list_elements_do_not_inflate_arity() {
    let src = "f(Node) ->\n\
               rpc:call(Node, rabbit_amqqueue, declare,\n\
                   [{resource, <<\"/\">>, queue, <<\"q1\">>}, true]).\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_amqqueue:declare/2".to_owned(),
            IndirectCallForm::RpcCall,
            2
        )]
    );
}

#[test]
fn a_form_behind_a_line_comment_extracts_nothing() {
    let src = "f() -> ok. % meck:expect(rabbit_queue_type, drain, 1, ok)\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

// Conditional compilation is not evaluated on added lines: a form
// between `-ifdef` and `-endif` is still a reference worth checking.
#[test]
fn a_form_inside_an_ifdef_region_still_extracts() {
    let src = "-ifdef(TEST).\n\
               setup() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n\
               -endif.\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_queue_type:drain/1".to_owned(),
            IndirectCallForm::MeckExpect,
            2
        )]
    );
}

#[test]
fn a_form_inside_a_spec_attribute_extracts_nothing() {
    let src = "-spec f() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
}

#[test]
fn meck_new_and_other_arities_are_not_recognized() {
    let src = "s(N) ->\n\
               meck:new(rabbit_queue_type, [no_link, passthrough]),\n\
               meck:expect(rabbit_queue_type, drain, 1, ok, extra),\n\
               rpc:call(N, rabbit_maintenance, drain).\n";
    let out = extract(src);
    assert_eq!(out.sites.len(), 0);
    assert_eq!(out.withheld_dynamic, 0);
}

#[test]
fn an_oversized_literal_list_counts_as_withheld() {
    let elements = vec!["x"; 300].join(", ");
    let src =
        format!("f(N) -> rpc:call(N, rabbit_misc, execute_mnesia_transaction, [{elements}]).\n");
    let out = extract(&src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

// The reason never blocks: a meck expectation can install a function
// the module never had, so absence proves drift, not failure.
#[test]
fn the_reason_is_non_blocking() {
    let reason = Reason::IndirectCallUndefinedOnTarget {
        source_path: rp("deps/rabbit/test/maintenance_mode_SUITE.erl"),
        module: module("rabbit_queue_type"),
        function: FunctionName::from_str("drain").unwrap(),
        arity: Arity::new(1),
        via: IndirectCallForm::MeckExpect,
        line: 124,
    };
    assert!(!reason.is_blocking());
    assert!(!reason.is_path_scoped());
    assert!(reason.apply_conflict().is_none());
}

// Resolution through `analyse_qualified_calls`.

/// `target` rows are `(module, path, file text)`.
fn analyse(
    added: &str,
    covered: &[&str],
    patch_added: &PatchProvided,
    target: &[(&str, &str, &str)],
) -> QualifiedCallAnalysis {
    let path = rp("deps/rabbit/test/maintenance_mode_SUITE.erl");
    let line_map: Vec<u32> = (1..=added.lines().count() as u32).collect();
    let subjects = [AddedLinesSubject {
        source_path: &path,
        added_text: added,
        line_map: &line_map,
    }];
    let module_to_path: BTreeMap<ModuleName, RelativePath> =
        target.iter().map(|(m, p, _)| (module(m), rp(p))).collect();
    let path_to_src: BTreeMap<String, String> = target
        .iter()
        .map(|(_, p, s)| ((*p).to_owned(), (*s).to_owned()))
        .collect();
    let covered: BTreeSet<ModuleName> = covered.iter().map(|s| module(s)).collect();
    let resolve = |m: &ModuleName| module_to_path.get(m).cloned();
    let read = |p: &RelativePath| path_to_src.get(p.as_str()).cloned();
    analyse_qualified_calls(&subjects, &covered, patch_added, &resolve, &read, None)
}

fn indirect_flags(reasons: &[Reason]) -> Vec<(String, String, u8, IndirectCallForm, u32)> {
    reasons
        .iter()
        .filter_map(|r| match r {
            Reason::IndirectCallUndefinedOnTarget {
                module,
                function,
                arity,
                via,
                line,
                ..
            } => Some((
                module.to_string(),
                function.to_string(),
                arity.get(),
                *via,
                *line,
            )),
            _ => None,
        })
        .collect()
}

const QUEUE_TYPE_WITHOUT_DRAIN: &str = "-module(rabbit_queue_type).\n\
                                        -export([init/0]).\n\
                                        init() -> ok.\n";

#[test]
fn a_mecked_function_absent_on_target_is_flagged() {
    let analysis = analyse(
        "t() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            QUEUE_TYPE_WITHOUT_DRAIN,
        )],
    );
    assert_eq!(
        indirect_flags(&analysis.reasons),
        [(
            "rabbit_queue_type".to_owned(),
            "drain".to_owned(),
            1,
            IndirectCallForm::MeckExpect,
            1
        )]
    );
    assert_eq!(analysis.indirect_checks.checked, 1);
}

#[test]
fn an_exported_function_is_not_flagged() {
    let analysis = analyse(
        "t() -> meck:expect(rabbit_queue_type, init, 0, ok).\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            QUEUE_TYPE_WITHOUT_DRAIN,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.indirect_checks.checked, 1);
}

// meck mocks unexported functions, so a defined-but-unexported callee
// is legitimate and stays unflagged.
#[test]
fn a_defined_but_unexported_function_is_not_flagged() {
    let target = "-module(rabbit_queue_type).\n\
                  -export([init/0]).\n\
                  init() -> internal_drain(x).\n\
                  internal_drain(_) -> ok.\n";
    let analysis = analyse(
        "t() -> meck:expect(rabbit_queue_type, internal_drain, 1, ok).\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            target,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.indirect_checks.checked, 1);
}

#[test]
fn a_function_the_patch_adds_is_not_flagged() {
    let patch_added = patch_provided(&[(
        module("rabbit_queue_type"),
        "-export([drain/1]).\ndrain(_) -> ok.\n",
    )]);
    let analysis = analyse(
        "t() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n",
        &[],
        &patch_added,
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            QUEUE_TYPE_WITHOUT_DRAIN,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
}

#[test]
fn a_snapshot_covered_module_is_withheld() {
    let analysis = analyse(
        "t() -> meck:expect(ra, members, 1, ok).\n",
        &["ra"],
        &PatchProvided::default(),
        &[("ra", "deps/ra/src/ra.erl", "-module(ra).\n")],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.indirect_checks.checked, 0);
}

#[test]
fn an_absent_module_is_withheld() {
    let analysis = analyse(
        "t() -> meck:expect(rabbit_queue_type, drain, 1, ok).\n",
        &[],
        &PatchProvided::default(),
        &[],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.indirect_checks.checked, 0);
}

#[test]
fn repeated_expectations_deduplicate_but_forms_stay_apart() {
    let analysis = analyse(
        "t(N) ->\n\
         meck:expect(rabbit_queue_type, drain, 1, ok),\n\
         meck:expect(rabbit_queue_type, drain, 1, again),\n\
         rpc:call(N, rabbit_queue_type, drain, [q]).\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            QUEUE_TYPE_WITHOUT_DRAIN,
        )],
    );
    let flags = indirect_flags(&analysis.reasons);
    assert_eq!(flags.len(), 2);
    assert_eq!(flags[0].3, IndirectCallForm::MeckExpect);
    assert_eq!(flags[1].3, IndirectCallForm::RpcCall);
    assert_eq!(analysis.indirect_checks.checked, 2);
}

#[test]
fn extraction_withholds_flow_into_the_tally() {
    let analysis = analyse(
        "t(N, Args) -> rpc:call(N, rabbit_queue_type, drain, Args).\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_queue_type",
            "deps/rabbit/src/rabbit_queue_type.erl",
            QUEUE_TYPE_WITHOUT_DRAIN,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.indirect_checks.withheld_dynamic, 1);
    assert_eq!(analysis.indirect_checks.checked, 0);
}
