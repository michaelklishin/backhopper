//! Human-readable formatting for the typed result structs in `result`.
//!
//! Kept separate from `result.rs` so the library data types stay free of
//! presentation concerns. Consumers who need machine output use
//! `serde::Serialize`; consumers who want text use these `Display` impls.

use std::fmt;

use crate::result::{
    AppDependencies, CalleesOf, CallersOf, DeprecatedFunctionCalls, ExportsNotUsed, LocalsNotUsed,
    ModuleCallers, ModuleDependencies, NonConformance, UndefinedFunctionCalls, UnresolvedCalls,
};

impl fmt::Display for UndefinedFunctionCalls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.entries {
            writeln!(f, "{} calls undefined {}", e.caller, e.callee)?;
        }
        Ok(())
    }
}

impl fmt::Display for LocalsNotUsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.entries {
            writeln!(f, "unused local: {}", e.mfa)?;
        }
        Ok(())
    }
}

impl fmt::Display for ExportsNotUsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.entries {
            let suffix = if e.satisfies_callback {
                " (callback)"
            } else if e.is_on_load {
                " (on_load)"
            } else {
                ""
            };
            writeln!(f, "unused export: {}{}", e.mfa, suffix)?;
        }
        Ok(())
    }
}

impl fmt::Display for DeprecatedFunctionCalls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.entries {
            writeln!(f, "{} calls deprecated {} ({})", e.caller, e.callee, e.tier)?;
        }
        Ok(())
    }
}

impl fmt::Display for CallersOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "callers of {}:", self.target)?;
        for c in &self.entries {
            writeln!(f, "  {}", c.caller)?;
        }
        Ok(())
    }
}

impl fmt::Display for CalleesOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "callees of {}:", self.source)?;
        for c in &self.entries {
            writeln!(f, "  {}", c.callee)?;
        }
        Ok(())
    }
}

impl fmt::Display for ModuleCallers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "callers of {}:", self.target)?;
        for m in &self.entries {
            writeln!(f, "  {}", m)?;
        }
        Ok(())
    }
}

impl fmt::Display for ModuleDependencies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} uses:", self.source)?;
        for m in &self.entries {
            writeln!(f, "  {}", m)?;
        }
        Ok(())
    }
}

impl fmt::Display for AppDependencies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} reaches:", self.source)?;
        for a in &self.entries {
            writeln!(f, "  {}", a)?;
        }
        Ok(())
    }
}

impl fmt::Display for NonConformance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} declares behaviour {} but is missing:",
            self.implementer, self.behaviour
        )?;
        for sig in &self.missing {
            writeln!(f, "  {}", sig)?;
        }
        Ok(())
    }
}

impl fmt::Display for UnresolvedCalls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for e in &self.entries {
            writeln!(f, "{} -> {}", e.caller, e.partial)?;
        }
        Ok(())
    }
}
