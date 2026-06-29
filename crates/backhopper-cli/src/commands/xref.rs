// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `backhopper xref ...` handler.

use backhopper_core::BehaviourName;

use crate::cli::{GlobalArgs, XrefCmd};
use crate::commands::tree_source::build_xref;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render, render_display};

fn ctx(global: &GlobalArgs, command: &'static str) -> OutputContext {
    OutputContext::new(global.formatter, command)
}

pub fn handle(global: &GlobalArgs, cmd: XrefCmd) -> CliResult<i32> {
    match cmd {
        XrefCmd::ListCallers {
            tree,
            mfa,
            transitive,
        } => {
            let xref = build_xref(&tree)?;
            let r = xref.called_by(&mfa, transitive);
            render_display(&ctx(global, "xref list_callers"), &r)
        }
        XrefCmd::ListCallees {
            tree,
            mfa,
            transitive,
        } => {
            let xref = build_xref(&tree)?;
            let r = xref.calls_from(&mfa, transitive);
            render_display(&ctx(global, "xref list_callees"), &r)
        }
        XrefCmd::ListUndefined { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.undefined_function_calls();
            render_display(&ctx(global, "xref list_undefined"), &r)
        }
        XrefCmd::ListUnusedExports { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.exports_not_used();
            render_display(&ctx(global, "xref list_unused_exports"), &r)
        }
        XrefCmd::ListUnusedLocals { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.locals_not_used();
            render_display(&ctx(global, "xref list_unused_locals"), &r)
        }
        XrefCmd::ListDeprecatedCalls { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.deprecated_function_calls();
            render_display(&ctx(global, "xref list_deprecated_calls"), &r)
        }
        XrefCmd::ListUnresolved { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.unresolved_calls();
            render_display(&ctx(global, "xref list_unresolved"), &r)
        }
        XrefCmd::ListModuleDeps {
            tree,
            module,
            forward,
        } => {
            let xref = build_xref(&tree)?;
            if forward {
                let r = xref.module_call(&module);
                render_display(&ctx(global, "xref list_module_deps"), &r)
            } else {
                let r = xref.module_called_by(&module);
                render_display(&ctx(global, "xref list_module_deps"), &r)
            }
        }
        XrefCmd::ListBehaviourUsers { tree, behaviour } => {
            let xref = build_xref(&tree)?;
            let beh =
                BehaviourName::new(behaviour).map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let r = xref.implementers_of(&beh);
            render(&ctx(global, "xref list_behaviour_users"), &r, |w| {
                for m in &r {
                    writeln!(w, "{m}")?;
                }
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListModuleCycles { tree } => {
            let xref = build_xref(&tree)?;
            let cycles = xref.module_cycles();
            render(&ctx(global, "xref list_module_cycles"), &cycles, |w| {
                for cycle in &cycles {
                    let joined: Vec<&str> = cycle.iter().map(|m| m.as_str()).collect();
                    writeln!(w, "{}", joined.join(" -> "))?;
                }
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::BackportApplicability {
            reference_file_path,
            snapshot_file_path,
        } => crate::commands::xref_backport_applicability::handle(
            global,
            &reference_file_path,
            &snapshot_file_path,
        ),
    }
}
