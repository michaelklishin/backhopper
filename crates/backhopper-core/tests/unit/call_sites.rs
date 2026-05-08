use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::extract_into;

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
