// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `extract_indirect_calls_elixir`: `m:f/a` references an Elixir
//! `:module.function(args)` call carries to a recognized rpc form, and
//! their resolution against the Erlang target tree through
//! `analyse_indirect_elixir_calls`.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use backhopper_core::compat::added_lines::AddedLinesSubject;
use backhopper_core::compat::indirect_calls::{IndirectExtraction, extract_indirect_calls_elixir};
use backhopper_core::compat::qualified_call_resolve::{
    IndirectCallAnalysis, PatchProvided, ReferenceCaches, ReferenceContext,
    analyse_indirect_elixir_calls, patch_provided,
};
use backhopper_core::model::names::{ModuleName, RelativePath};
use backhopper_core::model::verdict::{IndirectCallForm, Reason};

fn module(s: &str) -> ModuleName {
    ModuleName::from_str(s).unwrap()
}

fn rp(s: &str) -> RelativePath {
    RelativePath::new(s).unwrap()
}

fn extract(src: &str) -> IndirectExtraction {
    let line_map: Vec<u32> = (1..=src.lines().count() as u32).collect();
    extract_indirect_calls_elixir(src, &line_map)
}

fn sites(src: &str) -> Vec<(String, IndirectCallForm, u32)> {
    extract(src)
        .sites
        .into_iter()
        .map(|s| (s.mfa.to_string(), s.via, s.line))
        .collect()
}

// Extraction: the motivating incident and the shared table forms.

#[test]
fn the_plugins_cli_rpc_call_extracts() {
    let src = "def list(opts) do\n\
               :rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n\
               end\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_plugins:list/0".to_owned(),
            IndirectCallForm::RabbitMiscRpcCall,
            2
        )]
    );
}

#[test]
fn the_rpc_call_timeout_form_extracts() {
    let src = ":rabbit_misc.rpc_call(node, :rabbit_plugins, :enabled_plugins, [], 5000)\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_plugins:enabled_plugins/0".to_owned(),
            IndirectCallForm::RabbitMiscRpcCall,
            1
        )]
    );
}

#[test]
fn elixir_rpc_erpc_and_meck_forms_extract() {
    let src = ":rpc.call(node, :rabbit_maintenance, :drain, [])\n\
               :erpc.call(node, :rabbit_maintenance, :revive, [q], 5000)\n\
               :meck.expect(:rabbit_queue_type, :drain, 1, :ok)\n";
    assert_eq!(
        sites(src),
        [
            (
                "rabbit_maintenance:drain/0".to_owned(),
                IndirectCallForm::RpcCall,
                1
            ),
            (
                "rabbit_maintenance:revive/1".to_owned(),
                IndirectCallForm::ErpcCall,
                2
            ),
            (
                "rabbit_queue_type:drain/1".to_owned(),
                IndirectCallForm::MeckExpect,
                3
            ),
        ]
    );
}

#[test]
fn an_elixir_meck_expectation_through_the_rpc_helper_unwraps() {
    let src = ":rabbit_ct_broker_helpers.rpc(config, node, :meck, :expect, [:rabbit_queue_type, :drain, 1, :ok])\n";
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
fn quoted_elixir_atoms_extract() {
    let src = ":rabbit_misc.rpc_call(node, :\"rabbit_plugins\", :'list', [])\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_plugins:list/0".to_owned(),
            IndirectCallForm::RabbitMiscRpcCall,
            1
        )]
    );
}

#[test]
fn a_wrapped_multi_line_form_recovers_exact_arity() {
    let src = ":rabbit_misc.rpc_call(\n\
               node,\n\
               :rabbit_amqqueue,\n\
               :delete,\n\
               [q, false, false]\n\
               )\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_amqqueue:delete/3".to_owned(),
            IndirectCallForm::RabbitMiscRpcCall,
            1
        )]
    );
}

// Withholding and skips.

#[test]
fn a_bare_identifier_in_a_module_position_skips_without_counting() {
    let out = extract(":rabbit_misc.rpc_call(node, rabbit_plugins, :list, [])\n");
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

#[test]
fn a_variable_argument_list_counts_as_withheld() {
    let out = extract(":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, args)\n");
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

#[test]
fn a_cons_tail_counts_as_withheld() {
    let out = extract(":rpc.call(node, :rabbit_amqqueue, :lookup, [q | rest])\n");
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

// No Elixir fun grammar: a meck expectation with an anonymous function
// is a reference the axis saw and did not finish, counted not guessed.
#[test]
fn an_elixir_fun_arity_counts_as_withheld() {
    let out = extract(":meck.expect(:rabbit_khepri, :is_enabled, fn -> true end)\n");
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 1);
}

// Piping shifts every argument left by one, so a table position lands
// on a non-atom and the occurrence skips.
#[test]
fn a_piped_form_extracts_nothing() {
    let src = "node |> :rabbit_misc.rpc_call(:rabbit_plugins, :list, [], 5000)\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
}

#[test]
fn a_hash_comment_on_the_line_does_not_reach_the_matcher() {
    let src = "# :rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n";
    let out = extract(src);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

#[test]
fn an_unbalanced_paren_inside_a_string_does_not_break_the_scan() {
    let src = ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [\"a (b\"])\n";
    assert_eq!(
        sites(src),
        [(
            "rabbit_plugins:list/1".to_owned(),
            IndirectCallForm::RabbitMiscRpcCall,
            1
        )]
    );
}

#[test]
fn fragments_from_different_hunks_do_not_concatenate() {
    let src = ":rabbit_misc.rpc_call(\n\
               :rabbit_plugins,\n";
    let line_map = [10u32, 40];
    let out = extract_indirect_calls_elixir(src, &line_map);
    assert!(out.sites.is_empty());
    assert_eq!(out.withheld_dynamic, 0);
}

// Resolution through `analyse_indirect_elixir_calls`.

fn analyse(
    added: &str,
    covered: &[&str],
    patch_added: &PatchProvided,
    target: &[(&str, &str, &str)],
) -> IndirectCallAnalysis {
    let path = rp("deps/rabbitmq_cli/lib/rabbitmq/cli/plugins/plugins_helpers.ex");
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
    let ctx = ReferenceContext {
        covered_modules: &covered,
        patch_added,
        resolve_module_path: &resolve,
        read_target: &read,
        read_source: None,
    };
    let mut caches = ReferenceCaches::default();
    analyse_indirect_elixir_calls(&subjects, &ctx, &mut caches)
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

const PLUGINS_WITHOUT_LIST0: &str = "-module(rabbit_plugins).\n\
                                     -export([list/1]).\n\
                                     list(_) -> [].\n";

#[test]
fn an_rpcd_function_absent_on_target_is_flagged() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert_eq!(
        indirect_flags(&analysis.reasons),
        [(
            "rabbit_plugins".to_owned(),
            "list".to_owned(),
            0,
            IndirectCallForm::RabbitMiscRpcCall,
            1
        )]
    );
    assert_eq!(analysis.tally.checked, 1);
}

#[test]
fn an_exported_function_is_not_flagged() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [opts])\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.tally.checked, 1);
}

#[test]
fn a_snapshot_covered_module_is_withheld() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n",
        &["rabbit_plugins"],
        &PatchProvided::default(),
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.tally.checked, 0);
}

#[test]
fn an_absent_module_is_withheld() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n",
        &[],
        &PatchProvided::default(),
        &[],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.tally.checked, 0);
}

#[test]
fn a_function_the_patch_adds_is_not_flagged() {
    let patch_added = patch_provided(&[(
        module("rabbit_plugins"),
        "-export([list/0]).\nlist() -> [].\n",
    )]);
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n",
        &[],
        &patch_added,
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
}

#[test]
fn repeated_sites_deduplicate() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, [])\n\
         :rabbit_misc.rpc_call(other, :rabbit_plugins, :list, [])\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert_eq!(indirect_flags(&analysis.reasons).len(), 1);
    assert_eq!(analysis.tally.checked, 1);
}

#[test]
fn extraction_withholds_flow_into_the_tally() {
    let analysis = analyse(
        ":rabbit_misc.rpc_call(node, :rabbit_plugins, :list, args)\n",
        &[],
        &PatchProvided::default(),
        &[(
            "rabbit_plugins",
            "deps/rabbit/src/rabbit_plugins.erl",
            PLUGINS_WITHOUT_LIST0,
        )],
    );
    assert!(indirect_flags(&analysis.reasons).is_empty());
    assert_eq!(analysis.tally.withheld_dynamic, 1);
}
