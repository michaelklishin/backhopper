// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonical reader for the snapshot file format.
//!
//! Produces `Snapshot<Canonical>` only when the input parses *and* is in
//! canonical order. Non-canonical input is a hard error so we never
//! silently accept a hand-edited snapshot that has drifted out of order.

use std::str::FromStr;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::errors::{NameError, SnapshotError};
use crate::model::names::{
    Arity, CommitSha, FieldName, FunctionName, ModuleName, ProjectName, RecordName, TagName,
    TypeName,
};
use crate::model::snapshot::{
    ArityMatch, CallbackSig, Deprecation, DeprecationReplacement, FORMAT_VERSION, FunArity,
    HrlFile, Module, RecordDecl, RecordField, Snapshot, SnapshotHeader, SpecSig, TypeArity,
    TypeDecl, Visibility, state,
};
use crate::snapshot::SNAPSHOT_SIZE_LIMIT;

pub fn parse(input: &str) -> Result<Snapshot<state::Canonical>, SnapshotError> {
    if input.len() > SNAPSHOT_SIZE_LIMIT {
        return Err(SnapshotError::SizeLimit {
            size: input.len(),
            limit: SNAPSHOT_SIZE_LIMIT,
        });
    }
    let mut parser = Parser::new(input);
    parser.parse()
}

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>,
    cursor: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let lines = input.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
        Self { lines, cursor: 0 }
    }

    fn parse(&mut self) -> Result<Snapshot<state::Canonical>, SnapshotError> {
        let header = self.parse_header()?;
        let mut modules: Vec<Module> = Vec::new();
        let mut headers: Vec<HrlFile> = Vec::new();
        let mut headers_started = false;
        loop {
            self.skip_blank();
            let Some((lineno, line)) = self.peek() else {
                break;
            };
            if let Some(rest) = line.strip_prefix("module ") {
                if headers_started {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: "module after header block".into(),
                    });
                }
                self.advance();
                let name = ModuleName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if let Some(prev) = modules.last()
                    && name <= prev.name
                {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: format!("modules out of order at {}", name),
                    });
                }
                let module = self.parse_module_body(name)?;
                modules.push(module);
            } else if let Some(rest) = line.strip_prefix("header ") {
                headers_started = true;
                self.advance();
                let path = rest.trim().to_owned();
                if let Some(prev) = headers.last()
                    && path.as_str() <= prev.path.as_str()
                {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: format!("headers out of order at {}", path),
                    });
                }
                let hrl = self.parse_header_body(path)?;
                headers.push(hrl);
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: format!("expected 'module' or 'header', got {:?}", line),
                });
            }
        }
        Ok(Snapshot::from_canonical_parts(header, modules, headers))
    }

    fn parse_header(&mut self) -> Result<SnapshotHeader, SnapshotError> {
        let mut project: Option<ProjectName> = None;
        let mut tag: Option<TagName> = None;
        let mut branch: Option<String> = None;
        let mut commit: Option<CommitSha> = None;
        let mut scanned_paths: Vec<String> = Vec::new();
        let mut generated_by: Option<String> = None;
        let mut generated_at: Option<OffsetDateTime> = None;
        let mut format_version_seen = false;
        loop {
            let Some((lineno, line)) = self.peek() else {
                break;
            };
            let Some(content) = line.strip_prefix("# ") else {
                break;
            };
            self.advance();
            if content == "backhopper snapshot" {
                continue;
            }
            let (key, value) =
                content
                    .split_once(": ")
                    .ok_or_else(|| SnapshotError::MalformedHeader {
                        line: lineno,
                        detail: format!("missing ': ' in header line {:?}", line),
                    })?;
            match key {
                "format-version" => {
                    let parsed: u32 =
                        value.parse().map_err(|_| SnapshotError::MalformedHeader {
                            line: lineno,
                            detail: format!("non-integer format-version {:?}", value),
                        })?;
                    if parsed != FORMAT_VERSION {
                        return Err(SnapshotError::UnknownFormatVersion {
                            found: value.to_owned(),
                            expected: FORMAT_VERSION,
                        });
                    }
                    format_version_seen = true;
                }
                "project" => {
                    project = Some(ProjectName::from_str(value).map_err(SnapshotError::Name)?);
                }
                "tag" => {
                    tag = Some(TagName::from_str(value).map_err(SnapshotError::Name)?);
                }
                "branch" => {
                    branch = Some(value.to_owned());
                }
                "commit" => {
                    commit = Some(CommitSha::from_str(value).map_err(SnapshotError::Name)?);
                }
                "scanned-paths" => {
                    scanned_paths = value
                        .split(',')
                        .map(|p| p.trim().to_owned())
                        .filter(|p| !p.is_empty())
                        .collect();
                }
                "generated-by" => generated_by = Some(value.to_owned()),
                "generated-at" => {
                    generated_at = Some(OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
                        SnapshotError::MalformedHeader {
                            line: lineno,
                            detail: format!("invalid timestamp {:?}", value),
                        }
                    })?);
                }
                other => {
                    return Err(SnapshotError::UnknownHeaderKey {
                        line: lineno,
                        key: other.to_owned(),
                    });
                }
            }
        }
        if !format_version_seen {
            return Err(SnapshotError::MissingHeaderKey {
                key: "format-version",
            });
        }
        Ok(SnapshotHeader {
            project: project.ok_or(SnapshotError::MissingHeaderKey { key: "project" })?,
            tag: tag.ok_or(SnapshotError::MissingHeaderKey { key: "tag" })?,
            branch,
            commit: commit.ok_or(SnapshotError::MissingHeaderKey { key: "commit" })?,
            scanned_paths,
            generated_by: generated_by.ok_or(SnapshotError::MissingHeaderKey {
                key: "generated-by",
            })?,
            generated_at: generated_at.ok_or(SnapshotError::MissingHeaderKey {
                key: "generated-at",
            })?,
        })
    }

    fn parse_module_body(&mut self, name: ModuleName) -> Result<Module, SnapshotError> {
        let mut module = Module::new(name);
        let mut state = ClassOrder::default();
        loop {
            let Some((lineno, line)) = self.peek() else {
                break;
            };
            if !line.starts_with("  ") {
                break;
            }
            let trimmed = &line[2..];
            self.advance();
            if let Some(rest) = trimmed.strip_prefix("visibility ") {
                state.advance(EntryClass::Visibility, lineno)?;
                module.visibility = match rest.trim() {
                    "public" => Visibility::Public,
                    "hidden" => Visibility::Hidden,
                    "test_only" => Visibility::TestOnly,
                    other => {
                        return Err(SnapshotError::UnexpectedToken {
                            line: lineno,
                            detail: format!("unknown visibility {:?}", other),
                        });
                    }
                };
            } else if let Some(rest) = trimmed.strip_prefix("behaviour ") {
                state.advance(EntryClass::Behaviour, lineno)?;
                let m = ModuleName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if let Some(prev) = module.behaviours.last()
                    && &m <= prev
                {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: format!("behaviour {} out of order", m),
                    });
                }
                module.behaviours.push(m);
            } else if let Some(rest) = trimmed.strip_prefix("export ") {
                state.advance(EntryClass::Export, lineno)?;
                let fa = parse_fun_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_fun_arity_order(&module.exports, &fa, lineno, "export")?;
                module.exports.push(fa);
            } else if let Some(rest) = trimmed.strip_prefix("export_type ") {
                state.advance(EntryClass::ExportType, lineno)?;
                let ta = parse_type_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_type_arity_order(&module.export_types, &ta, lineno, "export_type")?;
                module.export_types.push(ta);
            } else if let Some(rest) = trimmed.strip_prefix("callback ") {
                state.advance(EntryClass::Callback, lineno)?;
                let (fa, sig) = parse_fun_arity_and_signature(rest).map_err(|e| {
                    SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: e,
                    }
                })?;
                let signature = self.collect_continuation(sig);
                module.callbacks.push(CallbackSig {
                    name: fa.name,
                    arity: fa.arity,
                    signature,
                });
            } else if let Some(rest) = trimmed.strip_prefix("optional_callback ") {
                state.advance(EntryClass::OptionalCallback, lineno)?;
                let fa = parse_fun_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_fun_arity_order(
                    &module.optional_callbacks,
                    &fa,
                    lineno,
                    "optional_callback",
                )?;
                module.optional_callbacks.push(fa);
            } else if let Some(rest) = trimmed.strip_prefix("spec ") {
                state.advance(EntryClass::Spec, lineno)?;
                let (fa, sig) = parse_fun_arity_and_signature(rest).map_err(|e| {
                    SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: e,
                    }
                })?;
                let signature = self.collect_continuation(sig);
                module.specs.push(SpecSig {
                    name: fa.name,
                    arity: fa.arity,
                    signature,
                });
            } else if let Some(rest) = trimmed.strip_prefix("type ") {
                state.advance(EntryClass::Type, lineno)?;
                let (ta, rhs) =
                    parse_type_decl_line(rest).map_err(|e| SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: e,
                    })?;
                let rhs_full = self.collect_continuation(rhs);
                module.types.push(TypeDecl {
                    name: ta.name,
                    arity: ta.arity,
                    rhs: rhs_full,
                });
            } else if let Some(rest) = trimmed.strip_prefix("opaque ") {
                state.advance(EntryClass::Opaque, lineno)?;
                let ta = parse_type_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_type_arity_order(&module.opaques, &ta, lineno, "opaque")?;
                module.opaques.push(ta);
            } else if let Some(rest) = trimmed.strip_prefix("deprecated ") {
                state.advance(EntryClass::Deprecated, lineno)?;
                let dep = parse_deprecation(rest).map_err(|e| SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: e,
                })?;
                module.deprecations.push(dep);
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: format!("unknown module entry {:?}", trimmed),
                });
            }
        }
        Ok(module)
    }

    fn parse_header_body(&mut self, path: String) -> Result<HrlFile, SnapshotError> {
        let mut hrl = HrlFile::new(path);
        let mut state = HrlClassOrder::default();
        loop {
            let Some((lineno, line)) = self.peek() else {
                break;
            };
            if !line.starts_with("  ") {
                break;
            }
            let trimmed = &line[2..];
            self.advance();
            if let Some(rest) = trimmed.strip_prefix("type ") {
                state.advance(HrlEntryClass::Type, lineno)?;
                let (ta, rhs) =
                    parse_type_decl_line(rest).map_err(|e| SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: e,
                    })?;
                let rhs_full = self.collect_continuation(rhs);
                hrl.types.push(TypeDecl {
                    name: ta.name,
                    arity: ta.arity,
                    rhs: rhs_full,
                });
            } else if let Some(rest) = trimmed.strip_prefix("opaque ") {
                state.advance(HrlEntryClass::Opaque, lineno)?;
                let ta = parse_type_arity(rest.trim()).map_err(SnapshotError::Name)?;
                hrl.opaques.push(ta);
            } else if let Some(rest) = trimmed.strip_prefix("record ") {
                state.advance(HrlEntryClass::Record, lineno)?;
                let name = RecordName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                let fields = self.parse_record_fields()?;
                hrl.records.push(RecordDecl { name, fields });
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: format!("unknown header entry {:?}", trimmed),
                });
            }
        }
        Ok(hrl)
    }

    fn parse_record_fields(&mut self) -> Result<Vec<RecordField>, SnapshotError> {
        let mut fields: Vec<RecordField> = Vec::new();
        while let Some((flineno, fline)) = self.peek() {
            if !fline.starts_with("    ") {
                break;
            }
            let ftrimmed = &fline[4..];
            if let Some(fbody) = ftrimmed.strip_prefix("field ") {
                self.advance();
                let (fname_str, ftype_repr) = match fbody.split_once(" :: ") {
                    Some((n, t)) => (n.trim(), Some(t.trim().to_owned())),
                    None => (fbody.trim(), None),
                };
                fields.push(RecordField {
                    name: FieldName::from_str(fname_str).map_err(SnapshotError::Name)?,
                    type_repr: ftype_repr,
                });
            } else if fline.starts_with("     ") {
                let Some(last) = fields.last_mut() else {
                    return Err(SnapshotError::UnexpectedToken {
                        line: flineno,
                        detail: format!("expected 'field', got {:?}", ftrimmed),
                    });
                };
                let Some(repr) = last.type_repr.as_mut() else {
                    return Err(SnapshotError::UnexpectedToken {
                        line: flineno,
                        detail: format!(
                            "type continuation but the previous field has no '::' type: {:?}",
                            fline,
                        ),
                    });
                };
                self.advance();
                repr.push('\n');
                repr.push_str(fline);
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: flineno,
                    detail: format!("expected 'field', got {:?}", ftrimmed),
                });
            }
        }
        Ok(fields)
    }

    fn collect_continuation(&mut self, first: String) -> String {
        let mut s = first;
        while let Some((_, line)) = self.peek()
            && line.starts_with("       ")
        {
            self.advance();
            s.push('\n');
            s.push_str(line);
        }
        s
    }

    fn peek(&self) -> Option<(usize, &'a str)> {
        self.lines.get(self.cursor).copied()
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn skip_blank(&mut self) {
        while let Some((_, line)) = self.peek() {
            if line.is_empty() {
                self.advance();
            } else {
                break;
            }
        }
    }
}

fn parse_fun_arity(s: &str) -> Result<FunArity, NameError> {
    let (n, a) = s
        .split_once('/')
        .ok_or_else(|| NameError::PatternMismatch {
            kind: "function/arity",
            value: s.to_owned(),
            pattern: "name/arity",
        })?;
    Ok(FunArity {
        name: FunctionName::from_str(n)?,
        arity: Arity::from_str(a)?,
    })
}

fn parse_type_arity(s: &str) -> Result<TypeArity, NameError> {
    let (n, a) = s
        .split_once('/')
        .ok_or_else(|| NameError::PatternMismatch {
            kind: "type/arity",
            value: s.to_owned(),
            pattern: "name/arity",
        })?;
    Ok(TypeArity {
        name: TypeName::from_str(n)?,
        arity: Arity::from_str(a)?,
    })
}

fn parse_fun_arity_and_signature(s: &str) -> Result<(FunArity, String), String> {
    let (head, sig) = s
        .split_once(' ')
        .ok_or_else(|| format!("expected 'name/arity body' in {:?}", s))?;
    let fa = parse_fun_arity(head).map_err(|e| e.to_string())?;
    Ok((fa, sig.to_owned()))
}

fn parse_type_decl_line(s: &str) -> Result<(TypeArity, String), String> {
    let (head, rhs) = s
        .split_once(" :: ")
        .ok_or_else(|| format!("expected 'name/arity :: rhs' in {:?}", s))?;
    let ta = parse_type_arity(head).map_err(|e| e.to_string())?;
    Ok((ta, rhs.to_owned()))
}

fn parse_deprecation(rest: &str) -> Result<Deprecation, String> {
    if rest.trim_start().starts_with("module") {
        return Ok(Deprecation {
            function: None,
            arity_match: ArityMatch::Any,
            since: None,
            replacement: None,
            reason: None,
            module_wide: true,
        });
    }
    let mut tokens = rest.split_whitespace().peekable();
    let head = tokens.next().ok_or("empty deprecation")?;
    let (function, arity_match) = if head == "*" {
        (None, ArityMatch::Any)
    } else {
        let (n, a) = head
            .split_once('/')
            .ok_or_else(|| format!("expected name/arity got {:?}", head))?;
        let arity_match = if a == "*" {
            ArityMatch::Any
        } else {
            ArityMatch::Exact {
                arity: Arity::from_str(a).map_err(|e| e.to_string())?,
            }
        };
        let function = FunctionName::from_str(n).map_err(|e| e.to_string())?;
        (Some(function), arity_match)
    };
    let mut since: Option<TagName> = None;
    let mut replacement: Option<DeprecationReplacement> = None;
    let mut reason: Option<String> = None;
    while let Some(tok) = tokens.next() {
        match tok {
            "since" => {
                let value = tokens.next().ok_or("missing 'since' value")?;
                since = Some(TagName::from_str(value).map_err(|e| e.to_string())?);
            }
            "use" => {
                let target = tokens.next().ok_or("missing 'use' target")?;
                let (n, a) = target
                    .split_once('/')
                    .ok_or_else(|| format!("expected use name/arity got {:?}", target))?;
                replacement = Some(DeprecationReplacement {
                    function: FunctionName::from_str(n).map_err(|e| e.to_string())?,
                    arity: Arity::from_str(a).map_err(|e| e.to_string())?,
                });
            }
            "reason" => {
                let remaining: Vec<&str> = tokens.collect();
                if remaining.is_empty() {
                    return Err("missing reason".into());
                }
                let joined = remaining.join(" ");
                let trimmed = joined
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(&joined);
                reason = Some(trimmed.to_owned());
                break;
            }
            other => return Err(format!("unexpected deprecation token {:?}", other)),
        }
    }
    Ok(Deprecation {
        function,
        arity_match,
        since,
        replacement,
        reason,
        module_wide: false,
    })
}

fn check_fun_arity_order(
    existing: &[FunArity],
    fa: &FunArity,
    lineno: usize,
    label: &str,
) -> Result<(), SnapshotError> {
    if let Some(prev) = existing.last() {
        let prev_key = (&prev.name, prev.arity);
        let new_key = (&fa.name, fa.arity);
        if new_key <= prev_key {
            return Err(SnapshotError::NotCanonical {
                line: lineno,
                detail: format!("{} {}/{} out of order", label, fa.name, fa.arity),
            });
        }
    }
    Ok(())
}

fn check_type_arity_order(
    existing: &[TypeArity],
    ta: &TypeArity,
    lineno: usize,
    label: &str,
) -> Result<(), SnapshotError> {
    if let Some(prev) = existing.last() {
        let prev_key = (&prev.name, prev.arity);
        let new_key = (&ta.name, ta.arity);
        if new_key <= prev_key {
            return Err(SnapshotError::NotCanonical {
                line: lineno,
                detail: format!("{} {}/{} out of order", label, ta.name, ta.arity),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Default)]
struct ClassOrder {
    last: Option<EntryClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntryClass {
    Visibility,
    Behaviour,
    Export,
    ExportType,
    Callback,
    OptionalCallback,
    Spec,
    Type,
    Opaque,
    Deprecated,
}

impl ClassOrder {
    fn advance(&mut self, c: EntryClass, line: usize) -> Result<(), SnapshotError> {
        if let Some(prev) = self.last
            && c < prev
        {
            return Err(SnapshotError::NotCanonical {
                line,
                detail: format!("entry-class order: {:?} after {:?}", c, prev),
            });
        }
        self.last = Some(c);
        Ok(())
    }
}

#[derive(Debug, Default)]
struct HrlClassOrder {
    last: Option<HrlEntryClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HrlEntryClass {
    Type,
    Opaque,
    Record,
}

impl HrlClassOrder {
    fn advance(&mut self, c: HrlEntryClass, line: usize) -> Result<(), SnapshotError> {
        if let Some(prev) = self.last
            && c < prev
        {
            return Err(SnapshotError::NotCanonical {
                line,
                detail: format!("hrl-entry-class order: {:?} after {:?}", c, prev),
            });
        }
        self.last = Some(c);
        Ok(())
    }
}
