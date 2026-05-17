//! `backhopper xref ...` handler.

use backhopper_core::BehaviourName;

use crate::cli::{GlobalArgs, XrefCmd};
use crate::commands::tree_source::build_xref;
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

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
            render(&ctx(global, "xref list_callers"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListCallees {
            tree,
            mfa,
            transitive,
        } => {
            let xref = build_xref(&tree)?;
            let r = xref.calls_from(&mfa, transitive);
            render(&ctx(global, "xref list_callees"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListUndefined { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.undefined_function_calls();
            render(&ctx(global, "xref list_undefined"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListUnusedExports { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.exports_not_used();
            render(&ctx(global, "xref list_unused_exports"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListUnusedLocals { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.locals_not_used();
            render(&ctx(global, "xref list_unused_locals"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListDeprecatedCalls { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.deprecated_function_calls();
            render(&ctx(global, "xref list_deprecated_calls"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListUnresolved { tree } => {
            let xref = build_xref(&tree)?;
            let r = xref.unresolved_calls();
            render(&ctx(global, "xref list_unresolved"), &r, |w| {
                write!(w, "{}", r)?;
                Ok(())
            })?;
            Ok(0)
        }
        XrefCmd::ListModuleDeps {
            tree,
            module,
            forward,
        } => {
            let xref = build_xref(&tree)?;
            if forward {
                let r = xref.module_call(&module);
                render(&ctx(global, "xref list_module_deps"), &r, |w| {
                    write!(w, "{}", r)?;
                    Ok(())
                })?;
            } else {
                let r = xref.module_called_by(&module);
                render(&ctx(global, "xref list_module_deps"), &r, |w| {
                    write!(w, "{}", r)?;
                    Ok(())
                })?;
            }
            Ok(0)
        }
        XrefCmd::ListBehaviourUsers { tree, behaviour } => {
            let xref = build_xref(&tree)?;
            let beh =
                BehaviourName::new(behaviour).map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let r = xref.implementers_of(&beh);
            render(&ctx(global, "xref list_behaviour_users"), &r, |w| {
                for m in &r {
                    writeln!(w, "{}", m)?;
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
    }
}
