// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `-spec` / `-callback` / `-type` body parsing.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignature {
    pub name: String,
    pub arity: u8,
    pub signature: String,
}

pub fn parse_callable_signature(body: &str) -> Option<ParsedSignature> {
    let trimmed = body.trim();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_string();
    let after_name = trimmed[name_end..].trim_start();
    if !after_name.starts_with('(') {
        return None;
    }
    let (args, rest_after_args) = take_balanced_parens(after_name)?;
    let arity = count_top_level_commas(args) + if args.trim().is_empty() { 0 } else { 1 };
    let rest = rest_after_args.trim_start();
    let after_arrow = rest.strip_prefix("->")?.trim_start();
    let signature = format!("{}({}) -> {}", name, args.trim(), after_arrow.trim());
    Some(ParsedSignature {
        name,
        arity: arity.min(255) as u8,
        signature,
    })
}

pub fn parse_type_decl(body: &str) -> Option<(String, u8, String)> {
    let trimmed = body.trim();
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_' && *c != '@' && *c != '\'')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    if name_end == 0 {
        return None;
    }
    let name = trimmed[..name_end].to_string();
    let after_name = trimmed[name_end..].trim_start();
    if !after_name.starts_with('(') {
        return None;
    }
    let (args, rest_after_args) = take_balanced_parens(after_name)?;
    let arity = count_top_level_commas(args) + if args.trim().is_empty() { 0 } else { 1 };
    let rest = rest_after_args.trim_start();
    let after_op = rest.strip_prefix("::")?.trim_start();
    Some((name, arity.min(255) as u8, after_op.trim().to_string()))
}

fn take_balanced_parens(s: &str) -> Option<(&str, &str)> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        let ch = b as char;
        if prev_back {
            prev_back = false;
            continue;
        }
        match ch {
            '\\' if in_string || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            '(' if !in_string && !in_atom_quote => depth += 1,
            ')' if !in_string && !in_atom_quote => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn count_top_level_commas(s: &str) -> usize {
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    let mut depth_brack = 0i32;
    // `<<...>>` bitstring types carry commas that are not argument
    // separators, e.g. `<<_:8, _:_*8>>` in a single spec argument.
    let mut depth_angle = 0i32;
    let mut in_string = false;
    let mut in_atom_quote = false;
    let mut prev_back = false;
    let mut count = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if prev_back {
            prev_back = false;
            continue;
        }
        match ch {
            '\\' if in_string || in_atom_quote => prev_back = true,
            '"' if !in_atom_quote => in_string = !in_string,
            '\'' if !in_string => in_atom_quote = !in_atom_quote,
            _ if in_string || in_atom_quote => {}
            '<' if chars.peek() == Some(&'<') => {
                chars.next();
                depth_angle += 1;
            }
            '>' if chars.peek() == Some(&'>') => {
                chars.next();
                depth_angle -= 1;
            }
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            '[' => depth_brack += 1,
            ']' => depth_brack -= 1,
            ',' if depth_paren == 0 && depth_brace == 0 && depth_brack == 0 && depth_angle == 0 => {
                count += 1;
            }
            _ => {}
        }
    }
    count
}
