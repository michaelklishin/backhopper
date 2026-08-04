// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Byte-level scanner tests for `compat::source_attributes`. Resolver
//! tests that need a real `TargetTreeIndex` are in
//! `integration/source_attributes_resolve_integration_tests.rs`.

use std::str::FromStr;

use backhopper_core::compat::call_sites::line_context;
use backhopper_core::compat::source_attributes::{
    ImportedFunction, declares_parse_transform, extract_behaviours, extract_defined_macro_values,
    extract_defined_macros, extract_defined_records, extract_exported_types,
    extract_function_signatures, extract_function_signatures_with_context, extract_imports,
    extract_includes, extract_macro_uses, extract_record_uses, extract_specs, is_predefined_macro,
};
use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::spec_ast::SpecType;
use backhopper_core::model::spec_parser::parse_signature_return;
use backhopper_core::model::symbol::RefContext;
use backhopper_core::model::verdict::IncludeDirective;

#[test]
fn record_uses_skip_maps_and_capture_names() {
    let uses = extract_record_uses("apply(S) -> S#state.machine;\ninit() -> #{leader => Pid}.\n");
    assert_eq!(
        uses.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
        ["state"]
    );
}

#[test]
fn defined_records_are_extracted() {
    let defs = extract_defined_records(
        "-record(state, {first_index, commit_index}).\n-record(cfg, {}).\n",
    );
    assert!(defs.contains("state"));
    assert!(defs.contains("cfg"));
}

#[test]
fn function_signatures_classify_definitions_and_calls() {
    let sigs = extract_function_signatures("apply(A) -> fold(A).\nfold(X) when X > 0 -> X.\n");
    let f = sigs.iter().find(|s| s.name == "apply").unwrap();
    assert_eq!((f.arity, f.is_definition), (1, true));
    let call = sigs
        .iter()
        .find(|s| s.name == "fold" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
    let guarded = sigs
        .iter()
        .find(|s| s.name == "fold" && s.is_definition)
        .unwrap();
    assert_eq!(guarded.arity, 1);
}

#[test]
fn a_binary_literal_argument_counts_as_one_argument() {
    // commas inside <<...>> separate binary segments, not call arguments:
    // id(<<1, 2, 3>>) is arity 1; arity 2 would fire a spurious flag against id/1
    let sigs = extract_function_signatures("dump(Entry) -> id(<<1, 2, 3>>).\nid(V) -> V.\n");
    let call = sigs
        .iter()
        .find(|s| s.name == "id" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
}

#[test]
fn a_variable_application_is_not_a_call() {
    // Fun(A, B) calls a variable, not a local function: its lowercase tail must not re-lex as un/2.
    let sigs = extract_function_signatures("apply(Fun) -> Fun(leader, follower).\n");
    assert!(sigs.iter().all(|s| s.name != "un"));
    assert!(sigs.iter().any(|s| s.name == "apply" && s.is_definition));
}

#[test]
fn a_variable_application_still_exposes_inner_calls() {
    // The variable is skipped whole, but a genuine call among its arguments is still found.
    let sigs = extract_function_signatures("apply(Fun, Q) -> Fun(queue_definition(Q)).\n");
    let call = sigs
        .iter()
        .find(|s| s.name == "queue_definition" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 1);
    assert!(sigs.iter().all(|s| s.name != "un"));
}

#[test]
fn an_attribute_form_is_not_a_call() {
    let sigs = extract_function_signatures("-export([members/1, process_command/2]).\n");
    assert!(sigs.is_empty());
}

#[test]
fn a_spec_type_is_not_a_call() {
    // map() and term() in a spec are type references, not zero-arity calls; the next clause still defines.
    let sigs = extract_function_signatures("-spec lookup(map()) -> term().\nlookup(M) -> M.\n");
    assert!(sigs.iter().all(|s| s.name != "map" && s.name != "term"));
    assert!(sigs.iter().any(|s| s.name == "lookup" && s.is_definition));
}

#[test]
fn subtraction_does_not_start_an_attribute() {
    // A mid-expression minus is an operator, not an attribute marker: the calls around it stay visible.
    let sigs = extract_function_signatures("total(X) -> high(X) - low(X).\n");
    assert!(sigs.iter().any(|s| s.name == "high" && !s.is_definition));
    assert!(sigs.iter().any(|s| s.name == "low" && !s.is_definition));
}

#[test]
fn a_define_body_call_is_not_a_call() {
    // Neither the define attribute name nor the call in its body is a local call here.
    let sigs = extract_function_signatures("-define(MAX, compute(1)).\n");
    assert!(
        sigs.iter()
            .all(|s| s.name != "define" && s.name != "compute")
    );
}

#[test]
fn a_call_after_an_attribute_is_still_found() {
    // The closing period ends the attribute: the following clause and its call scan normally.
    let sigs = extract_function_signatures("-spec start() -> ok.\nstart() -> real_call().\n");
    assert!(sigs.iter().any(|s| s.name == "start" && s.is_definition));
    assert!(
        sigs.iter()
            .any(|s| s.name == "real_call" && !s.is_definition)
    );
}

fn imported(module: &str, function: &str, arity: u8) -> ImportedFunction {
    ImportedFunction {
        module: ModuleName::from_str(module).unwrap(),
        function: FunctionName::from_str(function).unwrap(),
        arity: Arity::try_from(usize::from(arity)).unwrap(),
    }
}

#[test]
fn imports_are_extracted_with_module_name_and_arity() {
    let imps = extract_imports("-import(lists, [map/2, foldl/3]).\n");
    assert!(imps.contains(&imported("lists", "map", 2)));
    assert!(imps.contains(&imported("lists", "foldl", 3)));
}

#[test]
fn imports_from_distinct_modules_keep_their_own_module() {
    let imps = extract_imports(
        "-import(lists, [foldl/3]).\n-import(rabbit_data_coercion, [to_binary/1]).\n",
    );
    assert!(imps.contains(&imported("lists", "foldl", 3)));
    assert!(imps.contains(&imported("rabbit_data_coercion", "to_binary", 1)));
}

#[test]
fn parse_transform_is_detected() {
    assert!(declares_parse_transform(
        "-compile({parse_transform, lager_transform}).\n"
    ));
    assert!(!declares_parse_transform("-compile([debug_info]).\n"));
}

#[test]
fn macro_uses_capture_name_and_line_and_skip_comments_and_strings() {
    let src = "init() ->\n    %% ?COMMENTED\n    X = \"?QUOTED\",\n    ?REAL.\n";
    let uses = extract_macro_uses(src);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "REAL");
    assert_eq!(uses[0].line, 4);
}

#[test]
fn macro_use_stringify_form_names_the_macro() {
    let uses = extract_macro_uses("apply() -> ??NAME.\n");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "NAME");
}

#[test]
fn defined_macros_cover_constant_and_function_forms() {
    let src = "-define(DEFAULT_TIMEOUT, 1).\n-define(incr(X), X + 1).\n";
    let defs = extract_defined_macros(src);
    assert!(defs.contains("DEFAULT_TIMEOUT"));
    assert!(defs.contains("incr"));
}

#[test]
fn predefined_macros_are_recognised() {
    assert!(is_predefined_macro("MODULE"));
    assert!(is_predefined_macro("LINE"));
    assert!(!is_predefined_macro("OAUTH2_BOOTSTRAP_PATH"));
}

#[test]
fn extract_behaviours_finds_single_attribute() {
    let src = "-module(ra_server).\n-behaviour(gen_server).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "gen_server");
    assert_eq!(v[0].line, 2);
}

#[test]
fn extract_behaviours_accepts_us_spelling() {
    let src = "-behavior(gen_statem).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "gen_statem");
}

#[test]
fn extract_behaviours_handles_quoted_atom() {
    let src = "-behaviour('Quoted_atom').\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].behaviour.as_str(), "Quoted_atom");
}

#[test]
fn extract_behaviours_skips_in_comment_or_string() {
    let src = "% -behaviour(commented).\n\"-behaviour(\\\"string\\\")\".\n";
    let v = extract_behaviours(src);
    assert!(v.is_empty());
}

#[test]
fn extract_behaviours_records_line_for_each() {
    let src = "-module(ra_server).\n\n\n-behaviour(gen_server).\n-behaviour(supervisor).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].line, 4);
    assert_eq!(v[1].line, 5);
}

#[test]
fn extract_includes_recognises_both_forms() {
    let src = "-include(\"include/ra.hrl\").\n-include_lib(\"kernel/include/logger.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 2);
    assert!(matches!(
        v[0].directive,
        IncludeDirective::Include { ref path } if path == "include/ra.hrl"
    ));
    assert!(matches!(
        v[1].directive,
        IncludeDirective::IncludeLib { ref path } if path == "kernel/include/logger.hrl"
    ));
}

#[test]
fn extract_includes_skips_macro_value_form() {
    let src = "-include(?HEADER).\n";
    let v = extract_includes(src);
    assert!(v.is_empty());
}

#[test]
fn extract_includes_handles_attribute_with_extra_whitespace() {
    let src = "-include  (\n  \"osiris/osiris_log.hrl\"\n).\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].directive.path(), "osiris/osiris_log.hrl");
}

#[test]
fn extract_includes_skips_when_argument_is_not_a_string() {
    let src = "-include({rel, file}).\n";
    let v = extract_includes(src);
    assert!(v.is_empty());
}

#[test]
fn extract_includes_records_line_through_blank_lines() {
    let src = "\n\n-include(\"ra.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].line, 3);
}

#[test]
fn empty_source_yields_no_results() {
    assert!(extract_behaviours("").is_empty());
    assert!(extract_includes("").is_empty());
}

#[test]
fn extractors_do_not_panic_on_unmatched_parens() {
    let src = "-include(\"unfinished\n";
    let _ = extract_includes(src);
    let _ = extract_behaviours(src);
}

#[test]
fn extract_includes_finds_consecutive_attributes_on_separate_lines() {
    let src = "-include(\"ra.hrl\").\n-include(\"ra_server.hrl\").\n-include_lib(\"khepri/include/khepri.hrl\").\n";
    let v = extract_includes(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].directive.path(), "ra.hrl");
    assert_eq!(v[1].directive.path(), "ra_server.hrl");
    assert_eq!(v[2].directive.path(), "khepri/include/khepri.hrl");
}

#[test]
fn extract_behaviours_finds_three_in_a_row() {
    let src = "-behaviour(gen_server).\n-behaviour(supervisor).\n-behavior(gen_statem).\n";
    let v = extract_behaviours(src);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].behaviour.as_str(), "gen_server");
    assert_eq!(v[1].behaviour.as_str(), "supervisor");
    assert_eq!(v[2].behaviour.as_str(), "gen_statem");
}

// extract_specs

fn spec_key(function: &str, arity: u8) -> (FunctionName, Arity) {
    (FunctionName::from_str(function).unwrap(), Arity::new(arity))
}

#[test]
fn extract_specs_finds_a_single_spec() {
    let specs = extract_specs("-spec info(state()) -> [{atom(), integer()}].\n");
    assert_eq!(
        specs.get(&spec_key("info", 1)).unwrap(),
        "info(state()) -> [{atom(), integer()}]"
    );
}

#[test]
fn extract_specs_keeps_the_arrow_so_the_return_parses() {
    let specs = extract_specs("-spec segment_entry_count() -> pos_integer().\n");
    let sig = specs.get(&spec_key("segment_entry_count", 0)).unwrap();
    assert!(!matches!(parse_signature_return(sig), SpecType::Unknown));
}

#[test]
fn extract_specs_handles_a_multi_clause_spec() {
    let src = "-spec recover(term()) -> {ok, state()};\n       (undefined) -> {error, no_state}.\n";
    let specs = extract_specs(src);
    assert!(specs.contains_key(&spec_key("recover", 1)));
}

#[test]
fn extract_specs_handles_a_multi_line_body() {
    let src = "-spec ack([rabbit_variable_queue:seq_id()], State) ->\n    {[non_neg_integer()], State}\n    when State::state().\n";
    let specs = extract_specs(src);
    assert!(specs.contains_key(&spec_key("ack", 2)));
}

#[test]
fn extract_specs_handles_the_parenthesised_form() {
    let specs = extract_specs("-spec(delete_and_terminate(state()) -> state()).\n");
    assert!(specs.contains_key(&spec_key("delete_and_terminate", 1)));
}

#[test]
fn extract_specs_returns_empty_for_a_module_with_no_specs() {
    assert!(extract_specs("-module(rabbit_file).\ninit() -> ok.\n").is_empty());
}

#[test]
fn extract_specs_skips_module_qualified_forms() {
    let specs = extract_specs("-spec rabbit_misc:format(term(), list()) -> string().\n");
    assert!(specs.is_empty());
}

#[test]
fn extract_specs_does_not_overrun_into_the_next_form() {
    let src = "-spec info(state()) -> list().\n-type state() :: #qi{}.\n";
    let specs = extract_specs(src);
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs.get(&spec_key("info", 1)).unwrap(),
        "info(state()) -> list()"
    );
}

#[test]
fn extract_specs_ignores_spec_text_in_comments_and_strings() {
    let src = "%% -spec commented(term()) -> ok.\nformat() -> \"-spec fake(x) -> y.\".\n-spec real(term()) -> ok.\n";
    let specs = extract_specs(src);
    assert_eq!(specs.len(), 1);
    assert!(specs.contains_key(&spec_key("real", 1)));
}

#[test]
fn extract_specs_does_not_match_longer_attribute_names() {
    assert!(extract_specs("-specs_of(module) -> ok.\n").is_empty());
}

#[test]
fn extract_specs_bitstring_argument_keeps_the_snapshot_arity() {
    // The shared parser counts <<_:8, _:_*8>> as one argument; a divergent count would miskey and withhold.
    let specs = extract_specs("-spec encode(<<_:8, _:_*8>>) -> binary().\n");
    assert!(specs.contains_key(&spec_key("encode", 1)));
}

#[test]
fn extract_specs_is_not_derailed_by_a_char_literal_quote() {
    // $" is a char literal, not a string opener: the $ skip keeps the scanner
    // from consuming everything up to the next double quote, including the spec.
    let src = "quote() -> [$\", $-].\n-spec info(state()) -> list().\n";
    let specs = extract_specs(src);
    assert!(specs.contains_key(&spec_key("info", 1)));
}

// extract_defined_macro_values

#[test]
fn macro_values_capture_object_and_function_forms() {
    let src = "-define(SNAPSHOT_INTERVAL, 4096).\n-define(incr(X), X + 1).\n";
    let values = extract_defined_macro_values(src);
    let interval = &values["SNAPSHOT_INTERVAL"];
    assert_eq!(interval.len(), 1);
    assert_eq!(interval[0].params, None);
    assert_eq!(interval[0].body, "4096");
    let incr = &values["incr"];
    assert_eq!(incr[0].params, Some(1));
    assert_eq!(incr[0].body, "X + 1");
}

#[test]
fn macro_values_normalize_whitespace_and_comments() {
    let src = "-define(LIMITS(X), #{max => X,\n    %% cap\n    min => 0}).\n";
    let values = extract_defined_macro_values(src);
    assert_eq!(values["LIMITS"][0].body, "#{max => X, min => 0}");
}

#[test]
fn macro_values_keep_every_definition_of_a_name() {
    let src =
        "-ifdef(TEST).\n-define(LOG_LEVEL, debug).\n-else.\n-define(LOG_LEVEL, info).\n-endif.\n";
    let values = extract_defined_macro_values(src);
    assert_eq!(values["LOG_LEVEL"].len(), 2);
}

#[test]
fn macro_values_keep_commas_inside_the_body() {
    let src = "-define(PAIR, {a, b}).\n";
    let values = extract_defined_macro_values(src);
    assert_eq!(values["PAIR"][0].body, "{a, b}");
}

// A multi-line attribute body must not undercount the lines of following attributes.
#[test]
fn exported_type_line_survives_a_multiline_earlier_attribute() {
    let src = "-module(m).\n-export_type([\n    a/0,\n    b/0]).\n-export_type([c/0]).\n";
    let set = extract_exported_types(src);
    let c = set
        .types
        .iter()
        .find(|t| t.name.as_str() == "c")
        .expect("c/0 is exported");
    assert_eq!(c.line, 5);
}

#[test]
fn include_line_survives_a_multiline_string_literal() {
    let src = "-module(m).\n-define(BANNER, \"line one\nline two\").\n-include(\"a.hrl\").\n";
    let incs = extract_includes(src);
    assert_eq!(incs.len(), 1);
    assert_eq!(incs[0].line, 4);
}

// An added-lines blob can hold a `-spec` head whose terminating line
// stayed unchanged in the hunk: without the hunk-walked classification
// the byte tracking stays in the attribute and eats the clause heads.
#[test]
fn an_added_spec_head_does_not_swallow_following_definitions() {
    let blob = "-spec parse_props(binary(), protocol_version(), packet_type() | will_props) ->\n\
                parse_props(Bin, Vsn, _Type)\n\
                parse_props(<<0, Rest/binary>>, 5, _Type) ->\n\
                parse_props(Bin, 5, Type) ->\n";
    let ctx = [
        RefContext::TypeAttribute,
        RefContext::Body,
        RefContext::Body,
        RefContext::Body,
    ];
    let sigs = extract_function_signatures_with_context(blob, &ctx);
    let defs: Vec<_> = sigs
        .iter()
        .filter(|s| s.name == "parse_props" && s.is_definition)
        .collect();
    assert_eq!(defs.len(), 3);
    assert!(defs.iter().all(|s| s.arity == 3));
}

#[test]
fn an_orphaned_attribute_continuation_emits_no_signatures() {
    // continuation lines of a -spec whose opener stayed context
    let blob = "        rabbit_net:socket() | rabbit_net:proxy_socket(),\n\
                        keepalive_interval()) -> state().\n";
    let ctx = [RefContext::TypeAttribute, RefContext::TypeAttribute];
    let sigs = extract_function_signatures_with_context(blob, &ctx);
    assert!(sigs.is_empty(), "sigs: {sigs:?}");
}

#[test]
fn a_column_zero_head_without_a_visible_arrow_is_a_definition() {
    // the guard line carrying `when ... ->` stayed context and is absent
    let sigs = extract_function_signatures("parse_props(Bin, Vsn, _Type)\n");
    assert_eq!(sigs.len(), 1);
    assert!(sigs[0].is_definition);
    assert_eq!(sigs[0].arity, 3);
}

#[test]
fn an_indented_call_is_not_a_definition() {
    let sigs = extract_function_signatures("    parse_props(Bin, Vsn, Type)\n");
    assert_eq!(sigs.len(), 1);
    assert!(!sigs[0].is_definition);
}

#[test]
fn context_free_extraction_is_unchanged_on_full_files() {
    let src = "-module(rabbit_mqtt_packet).\n-export([parse/2]).\n-spec parse(binary(), integer()) ->\n    {ok, term()}.\nparse(Bin, Vsn)\n  when Vsn < 5 ->\n    parse_props(Bin, Vsn).\n";
    let sigs = extract_function_signatures(src);
    let def = sigs.iter().find(|s| s.name == "parse").unwrap();
    assert!(def.is_definition);
    assert_eq!((def.arity, def.line), (2, 5));
    let call = sigs.iter().find(|s| s.name == "parse_props").unwrap();
    assert!(!call.is_definition);
    assert_eq!((call.arity, call.line), (2, 7));
}

#[test]
fn a_local_call_with_an_inline_fun_argument_keeps_its_arity() {
    // the fun body's statement commas are not argument separators
    let src = "run(Props) ->\n    for_each_prop(\n      fun({Name, Val}) ->\n          assert_prop(Name, Val),\n          ok\n      end, Props).\n";
    let sigs = extract_function_signatures(src);
    let call = sigs
        .iter()
        .find(|s| s.name == "for_each_prop" && !s.is_definition)
        .unwrap();
    assert_eq!(call.arity, 2);
}

#[test]
fn signature_lines_survive_a_multiline_string_literal() {
    let src = "-define(BANNER, \"line one\nline two\").\nf(X) -> g(X).\n";
    let sigs = extract_function_signatures(src);
    let def = sigs.iter().find(|s| s.name == "f").unwrap();
    assert_eq!(def.line, 3);
}

// The self-derived classification and the byte tracking agree when the
// text holds every relevant line, so the two forms are interchangeable
// on full files.
#[test]
fn self_derived_context_matches_context_free_extraction() {
    let src = "-module(m).\n\
               -export([f/1]).\n\
               -spec f(integer()) ->\n\
                   integer().\n\
               f(X) ->\n\
                   g(X, X).\n";
    let ctx = line_context(src);
    assert_eq!(
        extract_function_signatures_with_context(src, &ctx),
        extract_function_signatures(src)
    );
}

#[test]
fn doc_triple_quoted_block_between_functions_yields_real_calls_only() {
    let src = r#"start() -> ra_directory:init().
-doc """
Prose that mentions fake_call(1) and half a "string.
""".
stop() -> ra_directory:deinit().
"#;
    let sigs = extract_function_signatures(src);
    let names: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["start", "stop"]);
}

#[test]
fn char_literal_quote_escape_does_not_perturb_later_signatures() {
    let src = "quote() -> $\\'.\nmembers(Cluster) -> Cluster.\n";
    let sigs = extract_function_signatures(src);
    let names: Vec<(&str, usize)> = sigs.iter().map(|s| (s.name.as_str(), s.arity)).collect();
    assert_eq!(names, vec![("quote", 0), ("members", 1)]);
}

#[test]
fn based_literal_hash_is_not_a_record_use() {
    let uses = extract_record_uses("mask() -> X = 16#ff, X.\n");
    assert!(uses.is_empty());
    let uses = extract_record_uses("mask() -> X = 16#ff, #ff{}.\n");
    let names: Vec<&str> = uses.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, vec!["ff"]);
}

#[test]
fn record_update_shapes_keep_their_references() {
    let uses = extract_record_uses("update(State2) -> State2#state{count = 1}.\n");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "state");
    let uses = extract_record_uses("update(X2) -> X2 #state{}.\n");
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].name, "state");
}

#[test]
fn based_float_exponent_hash_is_not_a_record_use() {
    let uses = extract_record_uses("scale() -> Y = 16#fe.fe#e16, Y.\n");
    assert!(uses.is_empty());
    let uses = extract_record_uses("bits() -> X = 2#1010, #state{}.\n");
    let names: Vec<&str> = uses.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, vec!["state"]);
}

#[test]
fn wildcard_and_variable_record_uses_report_no_name() {
    let uses = extract_record_uses("f(X) -> {#_{}, X#state{a = 1}, #'Weird'{}}.\n");
    let names: Vec<&str> = uses.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, vec!["state", "Weird"]);
}

#[test]
fn a_conditional_match_produces_no_phantom_macro_use() {
    let uses = extract_macro_uses("go(Id) ->\n    maybe Pid ?= find(Id), Pid end.\n");
    assert!(uses.is_empty());
}
