// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! `.app.src` discovery and parsing.
//!
//! Walks a source tree, finds every `*.app.src` file, and extracts
//! the application name, declared modules, and listed dependencies.
//! The parser is targeted: it accepts the small handful of Erlang
//! syntactic shapes that appear in real `.app.src` files and rejects
//! anything else as a parse error. It is not a general Erlang term
//! parser.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::names::{ApplicationName, ModuleName};

/// Parsed contents of one `.app.src` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSrcSpec {
    pub path: PathBuf,
    pub name: ApplicationName,
    pub vsn: Option<String>,
    pub modules: BTreeSet<ModuleName>,
    pub applications: BTreeSet<ApplicationName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryWarning {
    ParseFailed { path: PathBuf, reason: String },
    ReadFailed { path: PathBuf, reason: String },
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryOutput {
    pub specs: Vec<AppSrcSpec>,
    pub warnings: Vec<DiscoveryWarning>,
}

/// Directories the walker skips entirely. Common build-output and
/// vendored-dependency layouts: rebar3's `_build`, Erlang.mk's
/// `_rel` and `logs`, npm-style `node_modules`, Nix's `.direnv`,
/// plus `.git`. `target` is included so cross-language repos that
/// vendor Rust artifacts don't bloat the scan.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "_build",
    "_rel",
    "logs",
    "node_modules",
    ".direnv",
    "target",
];

/// Walks `root` recursively, parses every `*.app.src` found, returns
/// a `DiscoveryOutput` carrying the parsed specs plus any per-file
/// warnings.
pub fn discover(root: &Path) -> DiscoveryOutput {
    let mut out = DiscoveryOutput::default();
    walk_into(root, &mut out);
    out.specs.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk_into(dir: &Path, out: &mut DiscoveryOutput) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str())
                && SKIP_DIRS.contains(&name)
            {
                continue;
            }
            walk_into(&p, out);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if !is_app_src_path(&p) {
            continue;
        }
        match fs::read_to_string(&p) {
            Ok(text) => match parse(&p, &text) {
                Ok(spec) => out.specs.push(spec),
                Err(reason) => out
                    .warnings
                    .push(DiscoveryWarning::ParseFailed { path: p, reason }),
            },
            Err(err) => out.warnings.push(DiscoveryWarning::ReadFailed {
                path: p,
                reason: err.to_string(),
            }),
        }
    }
}

fn is_app_src_path(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".app.src") || n.ends_with(".app"))
}

/// Parses a single `.app.src` file's text. The accepted shape is
/// `{application, <name>, [<prop>, ...]}.` with optional whitespace,
/// `%`-line comments, and any prop order. Unknown props are ignored.
pub fn parse(path: &Path, text: &str) -> Result<AppSrcSpec, String> {
    let cleaned = strip_comments(text);
    let mut p = Parser::new(&cleaned);
    p.skip_ws();
    p.expect('{')?;
    p.skip_ws();
    let head = p.atom()?;
    if head != "application" {
        return Err(format!("expected 'application' tag, found '{head}'"));
    }
    p.skip_ws();
    p.expect(',')?;
    p.skip_ws();
    let app_name = p.atom()?;
    let app = ApplicationName::new(&app_name)
        .map_err(|e| format!("invalid application name '{app_name}': {e}"))?;
    p.skip_ws();
    p.expect(',')?;
    p.skip_ws();
    p.expect('[')?;
    let mut vsn: Option<String> = None;
    let mut modules: BTreeSet<ModuleName> = BTreeSet::new();
    let mut applications: BTreeSet<ApplicationName> = BTreeSet::new();
    loop {
        p.skip_ws();
        if p.peek() == Some(']') {
            p.advance();
            break;
        }
        if p.peek() == Some(',') {
            p.advance();
            continue;
        }
        if p.peek() != Some('{') {
            return Err(format!(
                "expected '{{' in props list at position {}",
                p.pos()
            ));
        }
        p.advance();
        p.skip_ws();
        let key = p.atom()?;
        p.skip_ws();
        p.expect(',')?;
        p.skip_ws();
        match key.as_str() {
            "vsn" => {
                if let Some(s) = p.try_string() {
                    vsn = Some(s);
                } else {
                    p.skip_value()?;
                }
            }
            "modules" => {
                if p.peek() == Some('[') {
                    let atoms = p.atom_list()?;
                    for a in atoms {
                        if let Ok(m) = ModuleName::new(&a) {
                            modules.insert(m);
                        }
                    }
                } else {
                    p.skip_value()?;
                }
            }
            "applications" => {
                if p.peek() == Some('[') {
                    let atoms = p.atom_list()?;
                    for a in atoms {
                        if let Ok(app) = ApplicationName::new(&a) {
                            applications.insert(app);
                        }
                    }
                } else {
                    p.skip_value()?;
                }
            }
            _ => {
                p.skip_value()?;
            }
        }
        p.skip_ws();
        p.expect('}')?;
    }
    p.skip_ws();
    p.expect('}')?;
    Ok(AppSrcSpec {
        path: path.to_path_buf(),
        name: app,
        vsn,
        modules,
        applications,
    })
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut prev_backslash = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if prev_backslash {
            out.push(ch);
            prev_backslash = false;
            continue;
        }
        if in_string {
            if ch == '\\' {
                out.push(ch);
                prev_backslash = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            out.push(ch);
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == '%' {
            while let Some(&next) = chars.peek() {
                if next == '\n' {
                    break;
                }
                chars.next();
            }
            continue;
        }
        out.push(ch);
    }
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            text,
            cursor: 0,
        }
    }

    fn pos(&self) -> usize {
        self.cursor
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.cursor).copied().map(|b| b as char)
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        match self.peek() {
            Some(c) if c == expected => {
                self.advance();
                Ok(())
            }
            Some(c) => Err(format!(
                "expected '{expected}' at position {}, found '{c}'",
                self.cursor
            )),
            None => Err(format!(
                "expected '{expected}' at position {}, found end of input",
                self.cursor
            )),
        }
    }

    fn atom(&mut self) -> Result<String, String> {
        let start = self.cursor;
        if self.peek() == Some('\'') {
            self.advance();
            let s = self.cursor;
            while let Some(c) = self.peek() {
                if c == '\'' {
                    let out = self.text[s..self.cursor].to_string();
                    self.advance();
                    return Ok(out);
                }
                self.advance();
            }
            return Err(format!(
                "unterminated quoted atom starting at position {start}"
            ));
        }
        let Some(first) = self.peek() else {
            return Err(format!("expected atom at position {start}"));
        };
        if !first.is_ascii_lowercase() {
            return Err(format!(
                "expected atom at position {start}, found '{first}'"
            ));
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '@' {
                self.advance();
            } else {
                break;
            }
        }
        Ok(self.text[start..self.cursor].to_string())
    }

    fn try_string(&mut self) -> Option<String> {
        if self.peek() != Some('"') {
            return None;
        }
        self.advance();
        let s = self.cursor;
        let mut prev_backslash = false;
        while let Some(c) = self.peek() {
            if prev_backslash {
                prev_backslash = false;
                self.advance();
                continue;
            }
            if c == '\\' {
                prev_backslash = true;
                self.advance();
                continue;
            }
            if c == '"' {
                let out = self.text[s..self.cursor].to_string();
                self.advance();
                return Some(out);
            }
            self.advance();
        }
        None
    }

    fn atom_list(&mut self) -> Result<Vec<String>, String> {
        self.expect('[')?;
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(']') => {
                    self.advance();
                    return Ok(out);
                }
                Some(',') => {
                    self.advance();
                }
                Some(c) if c == '\'' || c.is_ascii_lowercase() => {
                    out.push(self.atom()?);
                }
                Some(c) => {
                    return Err(format!(
                        "unexpected '{c}' in atom list at position {}",
                        self.cursor
                    ));
                }
                None => {
                    return Err(format!(
                        "unterminated atom list starting at position {}",
                        self.cursor
                    ));
                }
            }
        }
    }

    /// Skip past a single value: an atom, integer, string, list,
    /// tuple, or any nested combination. Stops at the matching depth.
    fn skip_value(&mut self) -> Result<(), String> {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut in_atom_quote = false;
        let mut prev_backslash = false;
        loop {
            let Some(ch) = self.peek() else {
                return Err(format!("unterminated value at position {}", self.cursor));
            };
            if prev_backslash {
                prev_backslash = false;
                self.advance();
                continue;
            }
            let in_quote = in_str || in_atom_quote;
            match ch {
                '\\' if in_quote => prev_backslash = true,
                '"' if !in_atom_quote => in_str = !in_str,
                '\'' if !in_str => in_atom_quote = !in_atom_quote,
                '[' | '{' | '(' if !in_quote => depth += 1,
                ']' | '}' | ')' if !in_quote => {
                    if depth == 0 {
                        return Ok(());
                    }
                    depth -= 1;
                }
                ',' if !in_quote && depth == 0 => return Ok(()),
                _ => {}
            }
            self.advance();
        }
    }
}