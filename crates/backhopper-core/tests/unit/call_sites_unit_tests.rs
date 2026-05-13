use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::{DynamicCall, extract_dynamic_into, extract_into};

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
