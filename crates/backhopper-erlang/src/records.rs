// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-record(name, {fields})` parser.

use crate::tokenizer::split_top_level_commas;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecord {
    pub name: String,
    pub fields: Vec<ParsedRecordField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecordField {
    pub name: String,
    pub type_repr: Option<String>,
}

pub fn parse_record(body: &str) -> Option<ParsedRecord> {
    let inside = body.trim();
    let inside = inside
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(inside)
        .trim();
    let parts = split_top_level_commas(inside);
    if parts.is_empty() {
        return None;
    }
    let name = parts[0].to_string();
    if name.is_empty() {
        return None;
    }
    let fields_text = parts[1..].join(",");
    let fields_inner = fields_text
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .map(str::trim)
        .unwrap_or("");
    let mut fields = Vec::new();
    for chunk in split_top_level_commas(fields_inner) {
        if let Some(field) = parse_record_field(chunk) {
            fields.push(field);
        }
    }
    Some(ParsedRecord { name, fields })
}

fn parse_record_field(s: &str) -> Option<ParsedRecordField> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_string();
    let rest = trimmed[name_end..].trim_start();
    if rest.is_empty() {
        return Some(ParsedRecordField {
            name,
            type_repr: None,
        });
    }
    let type_repr = rest.find("::").map(|idx| {
        let after_colon = rest[idx + 2..].trim();
        match after_colon.find('=') {
            Some(eq_idx) => after_colon[..eq_idx].trim().to_string(),
            None => after_colon.to_string(),
        }
    });
    Some(ParsedRecordField { name, type_repr })
}
