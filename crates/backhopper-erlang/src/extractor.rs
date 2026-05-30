// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Top-level Erlang source extractor: tokens → typed `Module` and `HrlFile` records.

use std::path::PathBuf;
use std::str::{self, FromStr};

use backhopper_core::model::names::{Arity, FieldName, FunctionName, RecordName, TypeName};
use backhopper_core::model::snapshot::{
    ArityMatch, CallbackSig, Deprecation, HrlFile, IfdefGuardKind, IfdefMacro, Module, RecordDecl,
    RecordField, SpecSig, TestExportVariant, TestOnlyExport, TypeArity, TypeDecl, VariantCBlock,
};
use backhopper_core::snapshot::spec_normalize::normalize_signature;
use thiserror::Error;
use tracing::warn;

use crate::attributes::{ParsedAttribute, classify};
use crate::cond_compile::CondStack;
use crate::deprecated::ParsedDeprecation;
use crate::specs::ParsedSignature;
use crate::tokenizer::iterate_attributes;
use crate::visibility::{
    VisibilityHints, classify as classify_visibility, detect_visibility_hints,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSource {
    pub modules: Vec<Module>,
    pub headers: Vec<HrlFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErlangExtractor {
    pub public_modules: Vec<String>,
    pub internal_modules: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("invalid utf-8 in {path:?} at byte offset {offset}")]
    InvalidUtf8 { path: PathBuf, offset: usize },
}

impl ErlangExtractor {
    pub fn new(public_modules: Vec<String>, internal_modules: Vec<String>) -> Self {
        Self {
            public_modules,
            internal_modules,
        }
    }

    pub fn extract_source<I>(&self, files: I) -> Result<ExtractedSource, ExtractError>
    where
        I: IntoIterator<Item = (PathBuf, Vec<u8>)>,
    {
        let mut modules: Vec<Module> = Vec::new();
        let mut headers: Vec<HrlFile> = Vec::new();
        for (path, bytes) in files {
            let text = str::from_utf8(&bytes).map_err(|e| ExtractError::InvalidUtf8 {
                path: path.clone(),
                offset: e.valid_up_to(),
            })?;
            let path_string = path.to_string_lossy().to_string();
            if path_string.ends_with(".hrl") {
                headers.push(self.extract_header_file(&path_string, text));
            } else if path_string.ends_with(".erl") {
                if let Some(m) = self.extract_module(text) {
                    modules.push(m);
                } else {
                    warn!("no -module attribute in {}", path_string);
                }
            }
        }
        Ok(ExtractedSource { modules, headers })
    }

    pub fn extract_module(&self, source: &str) -> Option<Module> {
        let blocks = iterate_attributes(source);
        let hints = detect_visibility_hints(source);
        let mut module: Option<Module> = None;
        let mut cond = CondStack::new();
        let mut region_stack: Vec<Option<PendingVariantC>> = Vec::new();
        let mut hidden_via_attr = false;
        let mut has_non_test_export = false;
        let mut has_only_test_exports = false;
        let mut test_only_exports: Vec<TestOnlyExport> = Vec::new();
        let mut ifdef_macros: Vec<IfdefMacro> = Vec::new();
        let mut variant_c_blocks: Vec<VariantCBlock> = Vec::new();
        for block in &blocks {
            if block.name == "define"
                && cond.is_test_only()
                && let Some(name) = parse_define_name(&block.body)
            {
                ifdef_macros.push(IfdefMacro {
                    name,
                    line: block.line,
                    guard_kind: IfdefGuardKind::Test,
                });
            }
            let Some(parsed) = classify(block) else {
                continue;
            };
            match parsed {
                ParsedAttribute::Module(name) => {
                    module = Some(Module::new(name));
                }
                ParsedAttribute::IfDef(ident) => {
                    cond.push_ifdef(&ident);
                    region_stack.push(None);
                }
                ParsedAttribute::IfnDef(ident) => {
                    cond.push_ifndef(&ident);
                    region_stack.push(if ident == "TEST" {
                        Some(PendingVariantC {
                            start_line: block.line,
                            else_line: None,
                        })
                    } else {
                        None
                    });
                }
                ParsedAttribute::If(expr) => {
                    cond.push_if(&expr);
                    region_stack.push(None);
                }
                ParsedAttribute::Else => {
                    cond.flip_else();
                    if let Some(Some(pending)) = region_stack.last_mut() {
                        pending.else_line = Some(block.line);
                    }
                }
                ParsedAttribute::EndIf => {
                    cond.pop_endif();
                    if let Some(Some(pending)) = region_stack.pop() {
                        variant_c_blocks.push(VariantCBlock {
                            guard: "TEST".to_owned(),
                            start_line: pending.start_line,
                            else_line: pending.else_line,
                            end_line: block.line,
                        });
                    }
                }
                ParsedAttribute::DocHidden => {
                    hidden_via_attr = true;
                }
                _ => {
                    let Some(m) = module.as_mut() else { continue };
                    let is_test = cond.is_test_only();
                    if let ParsedAttribute::Export(list) = &parsed {
                        if is_test {
                            has_only_test_exports = true;
                            for fa in list {
                                test_only_exports.push(TestOnlyExport {
                                    function: fa.name.clone(),
                                    arity: fa.arity,
                                    export_line: block.line,
                                    body_line: None,
                                    variant: TestExportVariant::A,
                                });
                            }
                        } else {
                            has_non_test_export = true;
                        }
                    }
                    apply_attribute(m, parsed);
                }
            }
        }
        let mut m = module?;
        let test_only = has_only_test_exports && !has_non_test_export;
        m.visibility = classify_visibility(
            &m.name,
            VisibilityHints {
                hidden: hidden_via_attr || hints.hidden,
            },
            test_only,
            &self.public_modules,
            &self.internal_modules,
        );
        m.clause_heads = crate::clause_heads::extract(source);
        m.test_only_exports = test_only_exports;
        m.ifdef_macros = ifdef_macros;
        m.variant_c_blocks = variant_c_blocks;
        Some(m)
    }

    pub fn extract_header_file(&self, path: &str, source: &str) -> HrlFile {
        let blocks = iterate_attributes(source);
        let mut hrl = HrlFile::new(path);
        for block in &blocks {
            let Some(parsed) = classify(block) else {
                continue;
            };
            match parsed {
                ParsedAttribute::Type {
                    name,
                    arity,
                    rhs,
                    opaque,
                } => {
                    let Ok(name) = TypeName::from_str(&name) else {
                        continue;
                    };
                    if opaque {
                        hrl.opaques.push(TypeArity {
                            name,
                            arity: Arity::new(arity),
                        });
                    } else {
                        hrl.types.push(TypeDecl {
                            name,
                            arity: Arity::new(arity),
                            rhs: normalize_signature(&rhs),
                        });
                    }
                }
                ParsedAttribute::Record(r) => {
                    let Ok(name) = RecordName::from_str(&r.name) else {
                        continue;
                    };
                    let mut fields = Vec::with_capacity(r.fields.len());
                    for f in r.fields {
                        let Ok(fname) = FieldName::from_str(&f.name) else {
                            continue;
                        };
                        fields.push(RecordField {
                            name: fname,
                            type_repr: f.type_repr,
                        });
                    }
                    hrl.records.push(RecordDecl { name, fields });
                }
                _ => {}
            }
        }
        hrl
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingVariantC {
    start_line: usize,
    else_line: Option<usize>,
}

fn parse_define_name(body: &str) -> Option<String> {
    let inner = body.trim_start().strip_prefix('(')?.trim_start();
    let end = inner.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    if end == 0 {
        return None;
    }
    Some(inner[..end].to_owned())
}

fn apply_attribute(m: &mut Module, attr: ParsedAttribute) {
    match attr {
        ParsedAttribute::Export(list) => m.exports.extend(list),
        ParsedAttribute::ExportType(list) => {
            for ta in list {
                if let Ok(name) = TypeName::from_str(&ta.name) {
                    m.export_types.push(TypeArity {
                        name,
                        arity: Arity::new(ta.arity),
                    });
                }
            }
        }
        ParsedAttribute::Behaviour(b) => m.behaviours.push(b),
        ParsedAttribute::Callback(sig) => {
            if let Some(c) = into_callback(sig) {
                m.callbacks.push(c);
            }
        }
        ParsedAttribute::OptionalCallbacks(list) => m.optional_callbacks.extend(list),
        ParsedAttribute::Spec(sig) => {
            if let Some(s) = into_spec(sig) {
                m.specs.push(s);
            }
        }
        ParsedAttribute::Type {
            name,
            arity,
            rhs,
            opaque,
        } => {
            if let Ok(n) = TypeName::from_str(&name) {
                if opaque {
                    m.opaques.push(TypeArity {
                        name: n,
                        arity: Arity::new(arity),
                    });
                } else {
                    m.types.push(TypeDecl {
                        name: n,
                        arity: Arity::new(arity),
                        rhs: normalize_signature(&rhs),
                    });
                }
            }
        }
        ParsedAttribute::Deprecated(list) => {
            for d in list {
                if let Some(deprecation) = into_deprecation(d) {
                    m.deprecations.push(deprecation);
                }
            }
        }
        _ => {}
    }
}

fn into_callback(sig: ParsedSignature) -> Option<CallbackSig> {
    Some(CallbackSig {
        name: FunctionName::from_str(&sig.name).ok()?,
        arity: Arity::new(sig.arity),
        signature: normalize_signature(&sig.signature),
    })
}

fn into_spec(sig: ParsedSignature) -> Option<SpecSig> {
    Some(SpecSig {
        name: FunctionName::from_str(&sig.name).ok()?,
        arity: Arity::new(sig.arity),
        signature: normalize_signature(&sig.signature),
    })
}

fn into_deprecation(d: ParsedDeprecation) -> Option<Deprecation> {
    if d.module_wide {
        return Some(Deprecation {
            function: None,
            arity_match: ArityMatch::Any,
            since: None,
            replacement: None,
            reason: None,
            module_wide: true,
        });
    }
    let function = d
        .function
        .as_deref()
        .and_then(|n| FunctionName::from_str(n).ok());
    let arity_match = if d.any_arity {
        ArityMatch::Any
    } else if let Some(a) = d.arity {
        ArityMatch::Exact {
            arity: Arity::new(a),
        }
    } else {
        ArityMatch::Any
    };
    Some(Deprecation {
        function,
        arity_match,
        since: None,
        replacement: None,
        reason: d.reason,
        module_wide: false,
    })
}
