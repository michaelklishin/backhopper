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
    let mut fields_text = String::new();
    let rest_joined = parts[1..].join(",");
    fields_text.push_str(rest_joined.trim());
    let fields_text_trimmed = fields_text.trim();
    let fields_inner = fields_text_trimmed
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
    let after_default = match rest.find('=') {
        Some(eq_idx) => {
            let before_eq = rest[..eq_idx].trim();
            before_eq.to_string()
        }
        None => rest.to_string(),
    };
    let type_repr = match after_default.find("::") {
        Some(idx) => Some(after_default[idx + 2..].trim().to_string()),
        None => None,
    };
    Some(ParsedRecordField { name, type_repr })
}
