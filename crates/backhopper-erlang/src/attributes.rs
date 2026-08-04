// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Translate raw attribute blocks into typed pieces of an Erlang module.

use std::str::FromStr;

use backhopper_core::model::names::{Arity, FunctionName, ModuleName};
use backhopper_core::model::snapshot::FunArity;

use crate::deprecated::{ParsedDeprecation, parse_deprecation_attribute};
use crate::records::{ParsedRecord, parse_record};
use crate::specs::{ParsedSignature, parse_callable_signature, parse_type_decl};
use crate::tokenizer::{
    AttributeBlock, split_top_level_commas, strip_outer_brackets, strip_outer_parens,
};
use backhopper_erlang_scan::remove_line_comments;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedAttribute {
    Module(ModuleName),
    Export(Vec<FunArity>),
    ExportType(Vec<TypeArityRaw>),
    Behaviour(ModuleName),
    Callback(ParsedSignature),
    OptionalCallbacks(Vec<FunArity>),
    Spec(ParsedSignature),
    Type {
        name: String,
        arity: u8,
        rhs: String,
        opaque: bool,
    },
    Record(ParsedRecord),
    Include(String),
    IncludeLib(String),
    Deprecated(Vec<ParsedDeprecation>),
    OnLoad(FunArity),
    DocHidden,
    IfDef(String),
    IfnDef(String),
    If(String),
    Else,
    EndIf,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeArityRaw {
    pub name: String,
    pub arity: u8,
}

pub fn classify(block: &AttributeBlock) -> Option<ParsedAttribute> {
    let body = strip_outer_parens(&block.body);
    match block.name.as_str() {
        "module" => ModuleName::from_str(body.trim())
            .ok()
            .map(ParsedAttribute::Module),
        "export" => Some(ParsedAttribute::Export(parse_fun_arity_list(body))),
        "export_type" => Some(ParsedAttribute::ExportType(parse_type_arity_list(body))),
        "behaviour" | "behavior" => ModuleName::from_str(body.trim())
            .ok()
            .map(ParsedAttribute::Behaviour),
        "callback" => parse_callable_signature(body).map(ParsedAttribute::Callback),
        "optional_callbacks" => Some(ParsedAttribute::OptionalCallbacks(parse_fun_arity_list(
            body,
        ))),
        "spec" => parse_callable_signature(body).map(ParsedAttribute::Spec),
        // nominal checking is name-based, which does not change the declared surface
        "type" | "nominal" => {
            parse_type_decl(body).map(|(name, arity, rhs)| ParsedAttribute::Type {
                name,
                arity,
                rhs,
                opaque: false,
            })
        }
        "opaque" => parse_type_decl(body).map(|(name, arity, rhs)| ParsedAttribute::Type {
            name,
            arity,
            rhs,
            opaque: true,
        }),
        "record" => parse_record(&block.body).map(ParsedAttribute::Record),
        "include" => Some(ParsedAttribute::Include(strip_string(body))),
        "include_lib" => Some(ParsedAttribute::IncludeLib(strip_string(body))),
        "deprecated" => Some(ParsedAttribute::Deprecated(parse_deprecation_attribute(
            &block.body,
        ))),
        "on_load" => parse_single_fun_arity(body).map(ParsedAttribute::OnLoad),
        "doc" => {
            if body.trim() == "hidden" {
                Some(ParsedAttribute::DocHidden)
            } else {
                Some(ParsedAttribute::Other(block.name.clone()))
            }
        }
        "ifdef" => Some(ParsedAttribute::IfDef(body.trim().to_owned())),
        "ifndef" => Some(ParsedAttribute::IfnDef(body.trim().to_owned())),
        "if" => Some(ParsedAttribute::If(body.trim().to_owned())),
        "else" => Some(ParsedAttribute::Else),
        "endif" => Some(ParsedAttribute::EndIf),
        _ => Some(ParsedAttribute::Other(block.name.clone())),
    }
}

fn parse_fun_arity_list(body: &str) -> Vec<FunArity> {
    let inside = strip_outer_brackets(body);
    let mut out = Vec::new();
    for chunk in split_top_level_commas(inside) {
        if let Some(fa) = parse_single_fun_arity(&remove_line_comments(chunk)) {
            out.push(fa);
        }
    }
    out
}

fn parse_type_arity_list(body: &str) -> Vec<TypeArityRaw> {
    let inside = strip_outer_brackets(body);
    let mut out = Vec::new();
    for chunk in split_top_level_commas(inside) {
        if let Some((n, a)) = remove_line_comments(chunk).split_once('/')
            && let Ok(arity) = a.trim().parse::<u8>()
        {
            out.push(TypeArityRaw {
                name: n.trim().to_owned(),
                arity,
            });
        }
    }
    out
}

fn parse_single_fun_arity(s: &str) -> Option<FunArity> {
    let (n, a) = s.split_once('/')?;
    let name = FunctionName::from_str(n.trim()).ok()?;
    let arity = Arity::from_str(a.trim()).ok()?;
    Some(FunArity { name, arity })
}

fn strip_string(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(trimmed)
        .to_owned()
}
