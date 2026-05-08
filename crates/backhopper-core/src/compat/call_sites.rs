//! Call-site extractor for Erlang source lines.
//!
//! Given a hunk line of source text, finds references to:
//!   * `mod:fun(args)` calls
//!   * `?MACRO_USE`
//!   * `#record_use{}`
//! and definitions:
//!   * `name(...)` heads on lines that start at column 0 (function clauses)

use std::str::FromStr;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::names::{Arity, FunctionName, Mfa, ModuleName, RecordName};
use crate::model::symbol::SymbolRef;

fn call_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"\b([a-z][a-zA-Z0-9_@]*)\s*:\s*([a-z][a-zA-Z0-9_@]*)\s*\(").expect("regex")
    })
}

fn record_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"#([a-z][a-zA-Z0-9_@]*)\b").expect("regex"))
}

fn macro_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\?([A-Z_][A-Za-z0-9_]*)\b").expect("regex"))
}

fn fun_def_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([a-z][a-zA-Z0-9_@]*)\s*\(").expect("regex"))
}

pub fn extract_into(line: &str, out: &mut Vec<SymbolRef>) {
    for caps in call_re().captures_iter(line) {
        let module = &caps[1];
        let function = &caps[2];
        let after = &line[caps.get(0).expect("capture").end()..];
        let arity = approximate_arity(after);
        if let (Ok(m), Ok(f)) = (
            ModuleName::from_str(module),
            FunctionName::from_str(function),
        ) {
            out.push(SymbolRef::function(Mfa::new(m, f, Arity::new(arity))));
        }
    }
    for caps in record_re().captures_iter(line) {
        if let Ok(name) = RecordName::from_str(&caps[1]) {
            out.push(SymbolRef::record(name));
        }
    }
    for caps in macro_re().captures_iter(line) {
        out.push(SymbolRef::macro_use(caps[1].to_owned()));
    }
}

pub fn extract_definitions_into(line: &str, out: &mut Vec<SymbolRef>) {
    if !line.starts_with(|c: char| c.is_ascii_lowercase()) {
        return;
    }
    let Some(caps) = fun_def_re().captures(line) else {
        return;
    };
    let head_end = caps.get(0).expect("capture").end();
    let after = &line[head_end..];
    let arity = approximate_arity(after);
    if let Ok(f) = FunctionName::from_str(&caps[1]) {
        let placeholder_module = ModuleName::from_str("_local").expect("valid");
        out.push(SymbolRef::function(Mfa::new(
            placeholder_module,
            f,
            Arity::new(arity),
        )));
    }
}

fn approximate_arity(after_open_paren: &str) -> u8 {
    let mut depth = 1i32;
    let mut count = 0u8;
    let mut saw_arg = false;
    let mut in_str = false;
    let mut in_atom_quote = false;
    let mut prev_backslash = false;
    for ch in after_open_paren.chars() {
        if prev_backslash {
            prev_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_str || in_atom_quote => prev_backslash = true,
            '"' if !in_atom_quote => in_str = !in_str,
            '\'' if !in_str => in_atom_quote = !in_atom_quote,
            '(' if !in_str && !in_atom_quote => depth += 1,
            ')' if !in_str && !in_atom_quote => {
                depth -= 1;
                if depth == 0 {
                    if saw_arg {
                        count = count.saturating_add(1);
                    }
                    return count;
                }
            }
            ',' if !in_str && !in_atom_quote && depth == 1 => {
                count = count.saturating_add(1);
                saw_arg = false;
            }
            c if !c.is_whitespace() && depth >= 1 => saw_arg = true,
            _ => {}
        }
    }
    count
}
