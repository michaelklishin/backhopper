// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::SymbolKind;
use backhopper_core::compat::arg_shape::ArgShape;
use backhopper_core::compat::call_sites::{
    DynamicCall, extract_call_args_into, extract_dynamic_into, extract_into,
};

#[test]
fn extracts_two_arg_call() {
    let mut out = Vec::new();
    extract_into(
        "    cowboy_req:set_resp_header(<<\"X\">>, <<\"Y\">>, Req)",
        &mut out,
    );
    let mfas: Vec<_> = out
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(mfas.iter().any(|m| m == "cowboy_req:set_resp_header/3"));
}

#[test]
fn extracts_zero_arg_call() {
    let mut out = Vec::new();
    extract_into("now() + erlang:system_time()", &mut out);
    let mfas: Vec<_> = out
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect();
    assert!(mfas.iter().any(|m| m == "erlang:system_time/0"));
}

#[test]
fn detects_record_use() {
    let mut out = Vec::new();
    extract_into("State#cfg.id", &mut out);
    let recs: Vec<_> = out
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Record { name } => Some(name.to_string()),
            _ => None,
        })
        .collect();
    assert!(recs.iter().any(|r| r == "cfg"));
}

#[test]
fn detects_macro_use() {
    let mut out = Vec::new();
    extract_into("?LOG_WARNING(\"hi\")", &mut out);
    let macros: Vec<_> = out
        .iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Macro { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(macros.iter().any(|m| m == "LOG_WARNING"));
}

#[test]
fn detects_apply_3_dispatch() {
    let mut out = Vec::new();
    extract_dynamic_into("    Result = apply(Mod, init, [Args])", &mut out);
    assert!(
        out.contains(&DynamicCall::Apply),
        "apply/3 should be flagged: {:?}",
        out
    );
}

#[test]
fn detects_spawn_family_calls() {
    let mut out = Vec::new();
    extract_dynamic_into("    Pid = spawn_link(Module, init, [Config])", &mut out);
    assert!(out.contains(&DynamicCall::Apply));
}

#[test]
fn detects_variable_module_dispatch() {
    let mut out = Vec::new();
    extract_dynamic_into("    Reply = Mod:handle_command(Cmd, State)", &mut out);
    assert!(
        out.contains(&DynamicCall::VariableDispatch),
        "Mod:fun should be flagged: {:?}",
        out
    );
}

#[test]
fn detects_variable_function_dispatch() {
    let mut out = Vec::new();
    extract_dynamic_into("    Result = osiris_log:F(Reader, Args)", &mut out);
    assert!(out.contains(&DynamicCall::VariableDispatch));
}

#[test]
fn literal_call_is_not_flagged_as_dynamic() {
    let mut out = Vec::new();
    extract_dynamic_into(
        "    {ok, Pid} = ra:start_server(Cfg, Conf, Servers)",
        &mut out,
    );
    assert!(
        out.is_empty(),
        "literal calls must not be classified as dynamic: {:?}",
        out
    );
}

#[test]
fn locally_defined_apply_lookalike_is_not_flagged() {
    let mut out = Vec::new();
    extract_dynamic_into("    Result = local_apply(Mod, Args)", &mut out);
    assert!(
        out.is_empty(),
        "local helper that ends in 'apply' must not trigger the BIF detector: {:?}",
        out
    );
}

#[test]
fn nested_dynamic_dispatch_counts_each_occurrence() {
    let mut out = Vec::new();
    extract_dynamic_into("    {Module:start(Cfg), Module:stop(Cfg)}", &mut out);
    assert_eq!(
        out.iter()
            .filter(|d| **d == DynamicCall::VariableDispatch)
            .count(),
        2
    );
}

#[test]
fn extract_call_args_classifies_atom_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:mode(start)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:mode/1");
    let (_, args) = entry.expect("ra:mode/1 captured");
    assert_eq!(
        args,
        &vec![ArgShape::Atom {
            name: "start".into(),
        }]
    );
}

#[test]
fn extract_call_args_classifies_variable_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:mode(Cmd)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:mode/1");
    let (_, args) = entry.expect("ra:mode/1 captured");
    assert_eq!(args, &vec![ArgShape::Variable]);
}

#[test]
fn extract_call_args_classifies_tuple_actuals_with_size() {
    let mut out = Vec::new();
    extract_call_args_into("ra:cast({register, Name, Pid})", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:cast/1");
    let (_, args) = entry.expect("ra:cast/1 captured");
    assert_eq!(args, &vec![ArgShape::Tuple { size: 3 }]);
}

#[test]
fn extract_call_args_classifies_record_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:apply(#cfg{id = X})", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:apply/1");
    let (_, args) = entry.expect("ra:apply/1 captured");
    let names: Vec<&str> = args
        .iter()
        .filter_map(|a| match a {
            ArgShape::Record { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["cfg"]);
}

#[test]
fn extract_call_args_handles_multiple_positional_arguments() {
    let mut out = Vec::new();
    extract_call_args_into("ra:new(start, 42, \"label\")", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:new/3");
    let (_, args) = entry.expect("ra:new/3 captured");
    assert_eq!(args.len(), 3);
    assert!(matches!(args[0], ArgShape::Atom { ref name } if name == "start"));
    assert!(matches!(args[1], ArgShape::Integer));
    assert!(matches!(args[2], ArgShape::String));
}

#[test]
fn extract_call_args_handles_zero_arity() {
    let mut out = Vec::new();
    extract_call_args_into("ra:init()", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:init/0");
    let (_, args) = entry.expect("ra:init/0 captured");
    assert!(args.is_empty());
}

#[test]
fn extract_call_args_classifies_list_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:items([1, 2, 3])", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:items/1");
    let (_, args) = entry.expect("ra:items/1 captured");
    assert_eq!(args, &vec![ArgShape::List]);
}

#[test]
fn extract_call_args_classifies_binary_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:emit(<<\"hello\">>)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:emit/1");
    let (_, args) = entry.expect("ra:emit/1 captured");
    assert_eq!(args, &vec![ArgShape::Binary]);
}

#[test]
fn extract_call_args_classifies_float_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:set(1.5)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:set/1");
    let (_, args) = entry.expect("ra:set/1 captured");
    assert_eq!(args, &vec![ArgShape::Float]);
}

#[test]
fn extract_call_args_classifies_negative_integer_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:set(-7)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:set/1");
    let (_, args) = entry.expect("ra:set/1 captured");
    assert_eq!(args, &vec![ArgShape::Integer]);
}

#[test]
fn extract_call_args_classifies_quoted_atom_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:emit('special atom')", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:emit/1");
    let (_, args) = entry.expect("ra:emit/1 captured");
    assert!(
        matches!(&args[0], ArgShape::Atom { name } if name == "special atom"),
        "args={:?}",
        args
    );
}

#[test]
fn extract_call_args_classifies_fun_reference_actuals() {
    let mut out = Vec::new();
    extract_call_args_into("ra:start(fun erlang:apply/2)", &mut out);
    let entry = out.iter().find(|(mfa, _)| mfa.to_string() == "ra:start/1");
    let (_, args) = entry.expect("ra:start/1 captured");
    assert_eq!(args, &vec![ArgShape::Fun]);
}