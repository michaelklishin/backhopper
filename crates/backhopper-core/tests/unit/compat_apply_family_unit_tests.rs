// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::SymbolKind;
use backhopper_core::compat::call_sites::{extract_into, extract_into_with_macros};
use backhopper_core::erlang_macros::{MacroKey, MacroTable};

fn mfas(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    extract_into(source, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

fn mfas_with_macros(source: &str, macros: &MacroTable) -> Vec<String> {
    let mut out = Vec::new();
    extract_into_with_macros(source, macros, &mut out);
    out.iter()
        .filter_map(|s| match &s.kind {
            SymbolKind::Function { mfa } => Some(mfa.to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn apply_3_with_atom_literals_resolves() {
    let out = mfas("    apply(worker, init, [a, b])");
    assert!(
        out.iter().any(|m| m == "worker:init/2"),
        "resolved apply target should appear: {out:?}"
    );
}

#[test]
fn spawn_3_with_atom_literals_resolves() {
    let out = mfas("spawn(worker, init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn spawn_link_3_with_atom_literals_resolves() {
    let out = mfas("spawn_link(worker, init, [])");
    assert!(out.iter().any(|m| m == "worker:init/0"), "{out:?}");
}

#[test]
fn spawn_monitor_3_with_atom_literals_resolves() {
    let out = mfas("spawn_monitor(worker, init, [a])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn spawn_4_strips_leading_node_argument() {
    let out = mfas("spawn(Node, worker, init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn spawn_opt_4_with_literals_resolves() {
    let out = mfas("spawn_opt(worker, init, [a], [link])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn spawn_opt_5_with_node_strips_leading_argument() {
    let out = mfas("spawn_opt(Node, worker, init, [a], [link])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn hibernate_3_with_literals_resolves() {
    let out = mfas("hibernate(worker, loop, [State])");
    assert!(out.iter().any(|m| m == "worker:loop/1"), "{out:?}");
}

#[test]
fn erlang_qualified_apply_resolves() {
    let out = mfas("erlang:apply(worker, init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn erlang_qualified_spawn_resolves() {
    let out = mfas("erlang:spawn(worker, init, [a, b])");
    assert!(out.iter().any(|m| m == "worker:init/2"), "{out:?}");
}

#[test]
fn apply_3_with_variable_module_does_not_resolve() {
    let out = mfas("apply(Mod, init, [cfg])");
    assert!(
        !out.iter().any(|m| m.contains(":init/")),
        "variable module should not resolve: {out:?}"
    );
}

#[test]
fn apply_3_with_variable_function_does_not_resolve() {
    let out = mfas("apply(worker, F, [cfg])");
    assert!(
        !out.iter()
            .any(|m| m == "worker:cfg/1" || m.starts_with("worker:")),
        "{out:?}"
    );
}

#[test]
fn apply_3_with_variable_args_does_not_resolve() {
    let out = mfas("apply(worker, init, Args)");
    assert!(!out.iter().any(|m| m.starts_with("worker:init")), "{out:?}");
}

#[test]
fn apply_2_does_not_resolve() {
    let out = mfas("apply(F, [cfg])");
    assert!(!out.iter().any(|m| m.contains("apply")), "{out:?}");
}

#[test]
fn macro_expansion_to_module_resolves() {
    let mut macros = MacroTable::new();
    macros.insert(
        MacroKey {
            name: "SERVER".into(),
            arity: None,
        },
        "my_module".into(),
    );
    let out = mfas_with_macros("?SERVER:start(1, 2)", &macros);
    assert!(out.iter().any(|m| m == "my_module:start/2"), "{out:?}");
}

#[test]
fn macro_expansion_to_mod_colon_fn_resolves() {
    let mut macros = MacroTable::new();
    macros.insert(
        MacroKey {
            name: "LOGGER".into(),
            arity: None,
        },
        "logger:log".into(),
    );
    let out = mfas_with_macros("?LOGGER(debug, \"hi\")", &macros);
    assert!(out.iter().any(|m| m == "logger:log/2"), "{out:?}");
}

#[test]
fn apply_3_with_macro_module_resolves() {
    let mut macros = MacroTable::new();
    macros.insert(
        MacroKey {
            name: "SERVER".into(),
            arity: None,
        },
        "my_module".into(),
    );
    let out = mfas_with_macros("apply(?SERVER, init, [cfg])", &macros);
    assert!(out.iter().any(|m| m == "my_module:init/1"), "{out:?}");
}

#[test]
fn nested_call_inside_apply_args_is_recorded() {
    let out = mfas("apply(worker, init, [extra:cfg()])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
    assert!(out.iter().any(|m| m == "extra:cfg/0"), "{out:?}");
}

#[test]
fn spawn_link_4_strips_leading_node_argument() {
    let out = mfas("spawn_link(Node, worker, init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn spawn_monitor_4_strips_leading_node_argument() {
    let out = mfas("spawn_monitor(Node, worker, init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}

#[test]
fn quoted_atom_module_argument_resolves() {
    let out = mfas("apply('worker', init, [cfg])");
    assert!(out.iter().any(|m| m == "worker:init/1"), "{out:?}");
}