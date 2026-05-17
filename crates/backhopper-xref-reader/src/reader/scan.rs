//! `ModuleBuilder` plus the top-level `scan` loop. Attribute parsing and
//! call-site detection live in sibling modules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use backhopper_core::{BehaviourName, ModuleName};
use backhopper_xref_graph::{Deprecation, FunctionSig, Loc, PathId, Position, UnresolvedCallSite};

use crate::application::ApplicationAssignment;
use crate::errors::ReadWarning;
use crate::macros::MacroTable;
use crate::model::{CallSite, ModuleData};
use crate::reader::attributes::{consume_attribute_body, handle_attribute};
use crate::reader::calls::{try_call_site, try_function_clause};
use crate::scanner::{Pos, Scanner};

pub(crate) struct ModuleBuilder {
    pub path: PathBuf,
    pub path_id: PathId,
    pub application: ApplicationAssignment,

    pub name: Option<ModuleName>,

    pub explicit_export_decls: BTreeSet<FunctionSig>,
    pub external_calls: Vec<CallSite>,
    pub local_calls: Vec<CallSite>,
    pub unresolved: BTreeSet<UnresolvedCallSite>,
    pub on_load: Option<FunctionSig>,
    pub deprecated: BTreeMap<FunctionSig, Deprecation>,
    pub behaviours: BTreeSet<BehaviourName>,
    pub callbacks: BTreeSet<FunctionSig>,
    pub optional_callbacks: BTreeSet<FunctionSig>,
    pub imports: BTreeMap<ModuleName, BTreeSet<FunctionSig>>,
    pub definitions: BTreeMap<FunctionSig, Loc>,
    pub macros: MacroTable,
    pub include_paths: Vec<PathBuf>,
}

impl ModuleBuilder {
    pub(crate) fn new(path: PathBuf, path_id: PathId, application: ApplicationAssignment) -> Self {
        Self {
            path,
            path_id,
            application,
            name: None,
            explicit_export_decls: BTreeSet::new(),
            external_calls: Vec::new(),
            local_calls: Vec::new(),
            unresolved: BTreeSet::new(),
            on_load: None,
            deprecated: BTreeMap::new(),
            behaviours: BTreeSet::new(),
            callbacks: BTreeSet::new(),
            optional_callbacks: BTreeSet::new(),
            imports: BTreeMap::new(),
            definitions: BTreeMap::new(),
            macros: MacroTable::new(),
            include_paths: Vec::new(),
        }
    }

    pub(crate) fn into_module(self, warnings: &mut Vec<ReadWarning>) -> Option<ModuleData> {
        let Some(name) = self.name else {
            warnings.push(ReadWarning::NoModuleAttribute {
                path: self.path.clone(),
            });
            return None;
        };
        let all_defs: BTreeSet<FunctionSig> = self.definitions.keys().cloned().collect();
        let exports = self
            .explicit_export_decls
            .intersection(&all_defs)
            .cloned()
            .collect();
        let locals: BTreeSet<FunctionSig> = all_defs.difference(&exports).cloned().collect();
        Some(ModuleData {
            module: name,
            application: self.application,
            exports,
            locals,
            external_calls: self.external_calls,
            local_calls: self.local_calls,
            unresolved: self.unresolved,
            on_load: self.on_load,
            deprecated: self.deprecated,
            behaviours: self.behaviours,
            callbacks: self.callbacks,
            optional_callbacks: self.optional_callbacks,
            imports: self.imports,
            definitions: self.definitions,
        })
    }

    pub(crate) fn set_module(&mut self, name: ModuleName, warnings: &mut Vec<ReadWarning>) {
        match &self.name {
            None => self.name = Some(name),
            Some(existing) if existing != &name => {
                warnings.push(ReadWarning::ConflictingModuleAttribute {
                    path: self.path.clone(),
                    first: existing.clone(),
                    second: name,
                });
            }
            _ => {}
        }
    }
}

pub(crate) fn pos_to_position(p: Pos) -> Position {
    Position::new(p.line, p.column, p.byte_offset)
}

pub(crate) fn span_loc(b: &ModuleBuilder, start: Pos, end: Pos) -> Loc {
    Loc::span(b.path_id, pos_to_position(start), pos_to_position(end))
}

/// Pass 1: walks the source for attributes only. Function bodies and
/// non-attribute syntax are skipped. The whole-module attributes that
/// influence later passes (`-module`, `-export`, `-define`,
/// `-include`/`-include_lib`, ...) land in the `ModuleBuilder` here.
pub(crate) fn scan_attributes(
    source: &str,
    b: &mut ModuleBuilder,
    warnings: &mut Vec<ReadWarning>,
) {
    let mut sc = Scanner::new(source);
    let mut at_line_start = true;
    while !sc.is_eof() {
        let Some(byte) = sc.peek() else { break };
        if byte == b'%' {
            sc.skip_line();
            at_line_start = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            if byte == b'\n' {
                at_line_start = true;
            }
            sc.advance();
            continue;
        }
        if at_line_start && byte == b'-' {
            sc.advance();
            let name = sc.consume_identifier().to_string();
            sc.skip_trivia();
            let body = consume_attribute_body(&mut sc);
            handle_attribute(&name, &body, b, warnings);
            at_line_start = false;
            continue;
        }
        at_line_start = false;
        match byte {
            b'\'' => sc.consume_quoted_atom(),
            b'"' => sc.consume_string(),
            b'$' => sc.consume_char_literal(),
            _ => {
                sc.advance();
            }
        }
    }
}

/// Folds the macros and nested includes contributed by a header file
/// into the host module's builder. Other attributes in the header are
/// ignored: headers contribute macros and records, never call sites.
pub(crate) fn fold_header_into(source: &str, b: &mut ModuleBuilder) {
    let mut sc = Scanner::new(source);
    let mut at_line_start = true;
    let mut sink: Vec<ReadWarning> = Vec::new();
    while !sc.is_eof() {
        let Some(byte) = sc.peek() else { break };
        if byte == b'%' {
            sc.skip_line();
            at_line_start = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            if byte == b'\n' {
                at_line_start = true;
            }
            sc.advance();
            continue;
        }
        if at_line_start && byte == b'-' {
            sc.advance();
            let name = sc.consume_identifier().to_string();
            sc.skip_trivia();
            let body = consume_attribute_body(&mut sc);
            if matches!(
                name.as_str(),
                "define" | "include" | "include_lib" | "record"
            ) {
                handle_attribute(&name, &body, b, &mut sink);
            }
            at_line_start = false;
            continue;
        }
        at_line_start = false;
        match byte {
            b'\'' => sc.consume_quoted_atom(),
            b'"' => sc.consume_string(),
            b'$' => sc.consume_char_literal(),
            _ => {
                sc.advance();
            }
        }
    }
}

/// Pass 2: walks the source for function clauses and call sites only.
/// Attribute lines are skipped because they've already been processed
/// by `scan_attributes`.
pub(crate) fn scan_bodies(source: &str, b: &mut ModuleBuilder) {
    let mut sc = Scanner::new(source);
    let mut current_function: Option<FunctionSig> = None;
    let mut at_line_start = true;

    while !sc.is_eof() {
        let pos = sc.pos();
        let Some(byte) = sc.peek() else { break };

        if byte == b'%' {
            sc.skip_line();
            at_line_start = true;
            continue;
        }
        if byte.is_ascii_whitespace() {
            if byte == b'\n' {
                at_line_start = true;
            }
            sc.advance();
            continue;
        }

        if at_line_start && byte == b'-' {
            sc.advance();
            let _name = sc.consume_identifier();
            sc.skip_trivia();
            let _ = consume_attribute_body(&mut sc);
            at_line_start = false;
            continue;
        }

        at_line_start = false;

        if pos.column == 1 && (byte.is_ascii_lowercase() || byte == b'_' || byte == b'\'') {
            if let Some(sig) = try_function_clause(&mut sc, pos, b) {
                current_function = Some(sig);
                continue;
            }
        }

        if byte == b'\'' {
            sc.consume_quoted_atom();
            continue;
        }
        if byte == b'"' {
            sc.consume_string();
            continue;
        }
        if byte == b'$' {
            sc.consume_char_literal();
            continue;
        }

        if let Some(caller) = current_function.as_ref() {
            if try_call_site(&mut sc, pos, caller, b) {
                continue;
            }
        }

        sc.advance();
    }
}
