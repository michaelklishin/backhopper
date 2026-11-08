// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonical reader for the snapshot file format.
//!
//! Produces `Snapshot<Canonical>` only when the input parses *and* is in
//! canonical order. Non-canonical input is a hard error so we never
//! silently accept a hand-edited snapshot that has drifted out of order.

use std::collections::BTreeSet;
use std::fmt::{Debug, Display};
use std::num::ParseIntError;
use std::str::FromStr;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::errors::{NameError, SnapshotError, UnknownLabel};
use crate::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable};
use crate::model::macro_decl::{MacroBody, MacroDecl, MacroKey};
use crate::model::names::{
    ApplicationName, Arity, CommitSha, ConfKey, DependencyName, DependencyVersion,
    ExtractionSettingsDigest, FieldName, FunctionName, HostFactsDigest, MacroName,
    MakeVariableName, ModuleName, PartialName, ProjectName, RecordName, RelativePath, TagName,
    TypeName, ValidatorName,
};
use crate::model::non_empty::NonEmpty;
use crate::model::normalised::{NormalisedDefault, NormalisedMacroBody};
use crate::model::schema_files::{PartialFile, SchemaFile};
use crate::model::snapshot::{
    ArityMatch, CallbackSig, Deprecation, DeprecationReplacement, FORMAT_VERSION, FunArity,
    HeaderRef, HostReleaseFacts, HrlFile, IfdefGuardKind, IfdefMacro, Module, Provenance,
    RecordDecl, RecordField, SUPPORTED_FORMAT_VERSIONS, Snapshot, SnapshotHeader, SnapshotParts,
    SpecSig, TestExportVariant, TestOnlyExport, TypeArity, TypeDecl, VariantCBlock, VendoredDep,
    VendoredDepSource, VersionedMachineVersion, Visibility, WireConstantBinding, WireValue, state,
};
use crate::snapshot::SNAPSHOT_SIZE_LIMIT;
use crate::snapshot::sort::breaks_order;

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

/// Parses only the leading `# ` header block, so a caller that needs the
/// provenance of a snapshot does not pay for its body.
pub fn parse_header(input: &str) -> Result<SnapshotHeader, SnapshotError> {
    Parser::new(header_block(input)).parse_header()
}

fn header_block(input: &str) -> &str {
    let mut end = 0;
    for line in input.split_inclusive('\n') {
        if !line.starts_with("# ") {
            break;
        }
        end += line.len();
    }
    &input[..end]
}

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>,
    cursor: usize,
    format_version: u32,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        let lines = input.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
        Self {
            lines,
            cursor: 0,
            format_version: 0,
        }
    }

    fn parse(&mut self) -> Result<Snapshot<state::Canonical>, SnapshotError> {
        let header = self.parse_header()?;
        self.format_version = header.format_version;
        let mut apps: Vec<AppManifest> = Vec::new();
        let mut modules: Vec<Module> = Vec::new();
        let mut headers: Vec<HrlFile> = Vec::new();
        let mut schemas: Vec<SchemaFile> = Vec::new();
        let mut partials: Vec<PartialFile> = Vec::new();
        let mut section = ClassOrder::new("section order");
        loop {
            self.skip_blank();
            let Some((lineno, line)) = self.peek() else {
                break;
            };
            if let Some(rest) = line.strip_prefix("app ") {
                self.require_format_5(lineno, "app section")?;
                section.advance(SectionClass::App, lineno)?;
                self.advance();
                let name = ApplicationName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if breaks_order(apps.last().map(|a| &a.name), &name) {
                    return Err(not_canonical(
                        lineno,
                        format!("apps out of order at {name}"),
                    ));
                }
                apps.push(self.parse_app_body(name)?);
            } else if let Some(rest) = line.strip_prefix("module ") {
                section.advance(SectionClass::Module, lineno)?;
                self.advance();
                let name = ModuleName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if breaks_order(modules.last().map(|m| &m.name), &name) {
                    return Err(not_canonical(
                        lineno,
                        format!("modules out of order at {name}"),
                    ));
                }
                let module = self.parse_module_body(name)?;
                if self.format_version >= 5 && module.app.is_none() {
                    return Err(missing_app(lineno, "module", &module.name));
                }
                modules.push(module);
            } else if let Some(rest) = line.strip_prefix("header ") {
                section.advance(SectionClass::Header, lineno)?;
                self.advance();
                let path = RelativePath::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if breaks_order(headers.last().map(|h| &h.path), &path) {
                    return Err(not_canonical(
                        lineno,
                        format!("headers out of order at {path}"),
                    ));
                }
                let hrl = self.parse_header_body(path)?;
                if self.format_version >= 5 && hrl.app.is_none() {
                    return Err(missing_app(lineno, "header", &hrl.path));
                }
                headers.push(hrl);
            } else if let Some(rest) = line.strip_prefix("schema ") {
                self.require_format_5(lineno, "schema section")?;
                section.advance(SectionClass::Schema, lineno)?;
                self.advance();
                let path = RelativePath::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if breaks_order(schemas.last().map(|s| &s.path), &path) {
                    return Err(not_canonical(
                        lineno,
                        format!("schemas out of order at {path}"),
                    ));
                }
                schemas.push(self.parse_schema_body(lineno, path)?);
            } else if let Some(rest) = line.strip_prefix("partial ") {
                self.require_format_5(lineno, "partial section")?;
                section.advance(SectionClass::Partial, lineno)?;
                self.advance();
                let path = RelativePath::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if breaks_order(partials.last().map(|p| &p.path), &path) {
                    return Err(not_canonical(
                        lineno,
                        format!("partials out of order at {path}"),
                    ));
                }
                partials.push(self.parse_partial_body(lineno, path)?);
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: format!(
                        "expected 'app', 'module', 'header', 'schema' or 'partial', got {line:?}"
                    ),
                });
            }
        }
        Ok(Snapshot::from_canonical_parts(SnapshotParts {
            header,
            apps,
            modules,
            headers,
            schemas,
            partials,
        }))
    }

    fn require_format_5(&self, line: usize, what: &str) -> Result<(), SnapshotError> {
        if self.format_version >= 5 {
            return Ok(());
        }
        Err(SnapshotError::UnexpectedToken {
            line,
            detail: format!(
                "{what} needs format-version 5, this file is format-version {}",
                self.format_version
            ),
        })
    }

    fn parse_header(&mut self) -> Result<SnapshotHeader, SnapshotError> {
        let mut project: Option<ProjectName> = None;
        let mut tag: Option<TagName> = None;
        let mut branch: Option<String> = None;
        let mut commit: Option<CommitSha> = None;
        let mut scanned_paths: Vec<String> = Vec::new();
        let mut apps_scanned: Vec<ApplicationName> = Vec::new();
        let mut generated_by: Option<String> = None;
        let mut generated_at: Option<OffsetDateTime> = None;
        let mut extractor_version: String = String::new();
        let mut dep_pins: Vec<VendoredDep> = Vec::new();
        let mut release_roots: Vec<ApplicationName> = Vec::new();
        let mut host_facts_digest: Option<HostFactsDigest> = None;
        let mut extraction_settings: Option<ExtractionSettingsDigest> = None;
        let mut format_version: u32 = 0;
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
                        detail: format!("missing ': ' in header line {line:?}"),
                    })?;
            match key {
                "format-version" => {
                    let parsed: u32 =
                        value.parse().map_err(|_| SnapshotError::MalformedHeader {
                            line: lineno,
                            detail: format!("non-integer format-version {value:?}"),
                        })?;
                    if !SUPPORTED_FORMAT_VERSIONS.contains(&parsed) {
                        return Err(SnapshotError::UnknownFormatVersion {
                            found: value.to_owned(),
                            expected: FORMAT_VERSION,
                        });
                    }
                    format_version = parsed;
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
                "apps-scanned" => {
                    apps_scanned = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ApplicationName::from_str)
                        .collect::<Result<_, _>>()
                        .map_err(SnapshotError::Name)?;
                }
                "generated-by" => generated_by = Some(value.to_owned()),
                "extractor-version" => value.clone_into(&mut extractor_version),
                "dep-pin" => {
                    dep_pins.push(parse_dep_pin_line(lineno, value)?);
                }
                "release-root" => {
                    require_header_format_5(format_version, lineno, key)?;
                    let root = ApplicationName::from_str(value).map_err(SnapshotError::Name)?;
                    if breaks_order(release_roots.last(), &root) {
                        return Err(not_canonical(
                            lineno,
                            format!("release-root {root} out of order"),
                        ));
                    }
                    release_roots.push(root);
                }
                "host-facts" => {
                    require_header_format_5(format_version, lineno, key)?;
                    host_facts_digest =
                        Some(HostFactsDigest::from_str(value).map_err(SnapshotError::Name)?);
                }
                "extraction-settings" => {
                    require_header_format_5(format_version, lineno, key)?;
                    extraction_settings = Some(
                        ExtractionSettingsDigest::from_str(value).map_err(SnapshotError::Name)?,
                    );
                }
                "generated-at" => {
                    generated_at = Some(OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
                        SnapshotError::MalformedHeader {
                            line: lineno,
                            detail: format!("invalid timestamp {value:?}"),
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
        let host_facts = match (NonEmpty::try_from(release_roots), host_facts_digest) {
            (Ok(release_roots), Some(digest)) => Some(HostReleaseFacts {
                release_roots,
                digest,
            }),
            (Err(_), None) => None,
            (Ok(_), None) => return Err(SnapshotError::MissingHeaderKey { key: "host-facts" }),
            (Err(_), Some(_)) => {
                return Err(SnapshotError::MissingHeaderKey {
                    key: "release-root",
                });
            }
        };
        Ok(SnapshotHeader {
            project: project.ok_or(SnapshotError::MissingHeaderKey { key: "project" })?,
            tag: tag.ok_or(SnapshotError::MissingHeaderKey { key: "tag" })?,
            branch,
            commit: commit.ok_or(SnapshotError::MissingHeaderKey { key: "commit" })?,
            scanned_paths,
            apps_scanned,
            generated_by: generated_by.ok_or(SnapshotError::MissingHeaderKey {
                key: "generated-by",
            })?,
            generated_at: generated_at.ok_or(SnapshotError::MissingHeaderKey {
                key: "generated-at",
            })?,
            extractor_version,
            format_version,
            dep_pins,
            host_facts,
            extraction_settings,
        })
    }

    fn parse_module_body(&mut self, name: ModuleName) -> Result<Module, SnapshotError> {
        let mut module = Module::new(name);
        let mut state = ClassOrder::new("entry-class order");
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
                let kw = rest.trim();
                module.visibility =
                    Visibility::from_keyword(kw).ok_or_else(|| SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: format!("unknown visibility {kw:?}"),
                    })?;
            } else if let Some(rest) = trimmed.strip_prefix("path ") {
                state.advance(EntryClass::Path, lineno)?;
                module.path =
                    Some(RelativePath::from_str(rest.trim()).map_err(SnapshotError::Name)?);
            } else if let Some(rest) = trimmed.strip_prefix("app ") {
                state.advance(EntryClass::App, lineno)?;
                let app = ApplicationName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                module.app = Some(app);
            } else if let Some(rest) = trimmed.strip_prefix("behaviour ") {
                state.advance(EntryClass::Behaviour, lineno)?;
                let m = ModuleName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                if let Some(prev) = module.behaviours.last()
                    && &m <= prev
                {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: format!("behaviour {m} out of order"),
                    });
                }
                module.behaviours.push(m);
            } else if let Some(rest) = trimmed.strip_prefix("export ") {
                state.advance(EntryClass::Export, lineno)?;
                let fa = parse_fun_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_order(&module.exports, &fa, lineno, "export")?;
                module.exports.push(fa);
            } else if let Some(rest) = trimmed.strip_prefix("export_type ") {
                state.advance(EntryClass::ExportType, lineno)?;
                let ta = parse_type_arity(rest.trim()).map_err(SnapshotError::Name)?;
                check_order(&module.export_types, &ta, lineno, "export_type")?;
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
                check_order(&module.optional_callbacks, &fa, lineno, "optional_callback")?;
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
                check_order(&module.opaques, &ta, lineno, "opaque")?;
                module.opaques.push(ta);
            } else if let Some(rest) = trimmed.strip_prefix("deprecated ") {
                state.advance(EntryClass::Deprecated, lineno)?;
                let dep = parse_deprecation(rest).map_err(|e| SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: e,
                })?;
                module.deprecations.push(dep);
            } else if let Some(rest) = trimmed.strip_prefix("record ") {
                state.advance(EntryClass::Record, lineno)?;
                let name = RecordName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                let fields = self.parse_record_fields()?;
                module.records.push(RecordDecl { name, fields });
            } else if let Some(rest) = trimmed.strip_prefix("test_only_export ") {
                state.advance(EntryClass::TestOnlyExport, lineno)?;
                let entry = parse_test_only_export(rest).map_err(|detail| {
                    SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail,
                    }
                })?;
                module.test_only_exports.push(entry);
            } else if let Some(rest) = trimmed.strip_prefix("ifdef_macro ") {
                state.advance(EntryClass::IfdefMacro, lineno)?;
                let entry =
                    parse_ifdef_macro(rest).map_err(|detail| SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail,
                    })?;
                module.ifdef_macros.push(entry);
            } else if let Some(rest) = trimmed.strip_prefix("variant_c_block ") {
                state.advance(EntryClass::VariantCBlock, lineno)?;
                let entry = parse_variant_c_block(rest).map_err(|detail| {
                    SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail,
                    }
                })?;
                module.variant_c_blocks.push(entry);
            } else if let Some(rest) = trimmed.strip_prefix("versioned_machine ") {
                state.advance(EntryClass::VersionedMachine, lineno)?;
                if module.versioned_machine_version.is_some() {
                    return Err(SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail: "duplicate versioned_machine entry".to_owned(),
                    });
                }
                let entry = parse_versioned_machine(rest).map_err(|detail| {
                    SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail,
                    }
                })?;
                module.versioned_machine_version = Some(entry);
            } else if let Some(rest) = trimmed.strip_prefix("wire_constant ") {
                state.advance(EntryClass::WireConstant, lineno)?;
                let entry =
                    parse_wire_constant(rest).map_err(|detail| SnapshotError::UnexpectedToken {
                        line: lineno,
                        detail,
                    })?;
                if let Some(prev) = module.wire_constants.last()
                    && entry.macro_name <= prev.macro_name
                {
                    return Err(SnapshotError::NotCanonical {
                        line: lineno,
                        detail: format!("wire_constant {} out of order", entry.macro_name),
                    });
                }
                module.wire_constants.push(entry);
            } else {
                return Err(SnapshotError::UnexpectedToken {
                    line: lineno,
                    detail: format!("unknown module entry {trimmed:?}"),
                });
            }
        }
        Ok(module)
    }

    fn parse_app_body(&mut self, name: ApplicationName) -> Result<AppManifest, SnapshotError> {
        let mut source: Option<ManifestSource> = None;
        let mut listed: Vec<ApplicationName> = Vec::new();
        let mut unreadable: Option<ManifestUnreadable> = None;
        let mut state = ClassOrder::new("app-entry class order");
        while let Some((lineno, trimmed)) = self.next_entry() {
            if let Some(rest) = trimmed.strip_prefix("manifest ") {
                state.advance(AppEntryClass::Manifest, lineno)?;
                if source.is_some() {
                    return Err(unexpected(lineno, "duplicate manifest line".into()));
                }
                source = Some(parse_label(lineno, rest)?);
            } else if let Some(rest) = trimmed.strip_prefix("requires ") {
                state.advance(AppEntryClass::Requires, lineno)?;
                let app = ApplicationName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                push_in_order(&mut listed, app, lineno, "requires")?;
            } else if let Some(rest) = trimmed.strip_prefix("requires_unreadable ") {
                state.advance(AppEntryClass::RequiresUnreadable, lineno)?;
                if unreadable.is_some() {
                    return Err(unexpected(
                        lineno,
                        "duplicate requires_unreadable line".into(),
                    ));
                }
                unreadable = Some(parse_unreadable(lineno, rest)?);
            } else {
                return Err(unexpected(lineno, format!("unknown app entry {trimmed:?}")));
            }
        }
        let source = source.ok_or(SnapshotError::MissingHeaderKey { key: "manifest" })?;
        let requires = match unreadable {
            Some(reason) if listed.is_empty() => ManifestDeps::Unreadable(reason),
            Some(_) => {
                return Err(unexpected(
                    self.cursor,
                    format!("app {name} lists requirements and marks them unreadable"),
                ));
            }
            None => ManifestDeps::Listed(listed.into_iter().collect()),
        };
        Ok(AppManifest {
            name,
            source,
            requires,
        })
    }

    fn parse_schema_body(
        &mut self,
        start: usize,
        path: RelativePath,
    ) -> Result<SchemaFile, SnapshotError> {
        let (app, _, cuttlefish) = self.parse_cuttlefish_body(start, false)?;
        Ok(SchemaFile {
            path,
            app,
            mappings: cuttlefish.mappings,
            validators: cuttlefish.validators,
        })
    }

    fn parse_partial_body(
        &mut self,
        start: usize,
        path: RelativePath,
    ) -> Result<PartialFile, SnapshotError> {
        let (app, name, cuttlefish) = self.parse_cuttlefish_body(start, true)?;
        let name = name.ok_or_else(|| missing_line(start, "partial", &path, "name"))?;
        Ok(PartialFile {
            path,
            app,
            name,
            mappings: cuttlefish.mappings,
            validators: cuttlefish.validators,
        })
    }

    fn parse_cuttlefish_body(
        &mut self,
        start: usize,
        named: bool,
    ) -> Result<(ApplicationName, Option<PartialName>, CuttlefishEntries), SnapshotError> {
        let mut state = ClassOrder::new("cuttlefish-entry class order");
        let mut app: Option<ApplicationName> = None;
        let mut name: Option<PartialName> = None;
        let mut mappings: Vec<ConfKey> = Vec::new();
        let mut validators: Vec<ValidatorName> = Vec::new();
        while let Some((lineno, trimmed)) = self.next_entry() {
            if let Some(rest) = trimmed.strip_prefix("app ") {
                state.advance(CuttlefishEntryClass::App, lineno)?;
                app = Some(ApplicationName::from_str(rest.trim()).map_err(SnapshotError::Name)?);
            } else if named && let Some(rest) = trimmed.strip_prefix("name ") {
                state.advance(CuttlefishEntryClass::Name, lineno)?;
                name = Some(PartialName::from_str(rest.trim()).map_err(SnapshotError::Name)?);
            } else if let Some(rest) = trimmed.strip_prefix("mapping ") {
                state.advance(CuttlefishEntryClass::Mapping, lineno)?;
                let key = ConfKey::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                push_in_order(&mut mappings, key, lineno, "mapping")?;
            } else if let Some(rest) = trimmed.strip_prefix("validator ") {
                state.advance(CuttlefishEntryClass::Validator, lineno)?;
                let validator =
                    ValidatorName::from_str(rest.trim()).map_err(SnapshotError::Name)?;
                push_in_order(&mut validators, validator, lineno, "validator")?;
            } else {
                return Err(unexpected(
                    lineno,
                    format!("unknown cuttlefish entry {trimmed:?}"),
                ));
            }
        }
        let app =
            app.ok_or_else(|| unexpected(start, "cuttlefish section has no app line".to_owned()))?;
        let entries = CuttlefishEntries {
            mappings: mappings.into_iter().collect(),
            validators: validators.into_iter().collect(),
        };
        Ok((app, name, entries))
    }

    fn parse_header_body(&mut self, path: RelativePath) -> Result<HrlFile, SnapshotError> {
        let mut hrl = HrlFile::new(path);
        let mut state = ClassOrder::new("hrl-entry-class order");
        let mut defines: Vec<MacroDecl> = Vec::new();
        while let Some((lineno, trimmed)) = self.next_entry() {
            if let Some(rest) = trimmed.strip_prefix("app ") {
                self.require_format_5(lineno, "header app line")?;
                state.advance(HrlEntryClass::App, lineno)?;
                hrl.app =
                    Some(ApplicationName::from_str(rest.trim()).map_err(SnapshotError::Name)?);
            } else if let Some(rest) = trimmed.strip_prefix("include ") {
                self.require_format_5(lineno, "include line")?;
                state.advance(HrlEntryClass::Include, lineno)?;
                let include = parse_header_ref(lineno, rest)?;
                push_in_order(&mut hrl.includes, include, lineno, "include")?;
            } else if let Some(rest) = trimmed.strip_prefix("type ") {
                state.advance(HrlEntryClass::Type, lineno)?;
                let (ta, rhs) =
                    parse_type_decl_line(rest).map_err(|detail| unexpected(lineno, detail))?;
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
            } else if let Some(rest) = trimmed.strip_prefix("define ") {
                self.require_format_5(lineno, "define line")?;
                state.advance(HrlEntryClass::Define, lineno)?;
                let decl = parse_define_line(lineno, rest)?;
                if breaks_order(defines.last().map(|d| &d.key), &decl.key) {
                    return Err(not_canonical(
                        lineno,
                        format!("define {} out of order", decl.key),
                    ));
                }
                defines.push(decl);
            } else {
                return Err(unexpected(
                    lineno,
                    format!("unknown header entry {trimmed:?}"),
                ));
            }
        }
        hrl.defines = defines;
        Ok(hrl)
    }

    fn parse_record_fields(&mut self) -> Result<Vec<RecordField>, SnapshotError> {
        let mut raw: Vec<RawField> = Vec::new();
        while let Some((flineno, fline)) = self.peek() {
            if !fline.starts_with("    ") {
                break;
            }
            let ftrimmed = &fline[4..];
            if let Some(fbody) = ftrimmed.strip_prefix("field ") {
                self.advance();
                raw.push(self.parse_field_line(flineno, fbody)?);
            } else if fline.starts_with("     ") {
                let Some(last) = raw.last_mut() else {
                    return Err(unexpected(
                        flineno,
                        format!("expected 'field', got {ftrimmed:?}"),
                    ));
                };
                let Some(repr) = last.type_text.as_mut() else {
                    return Err(unexpected(
                        flineno,
                        format!(
                            "type continuation but the previous field has no '::' type: {fline:?}",
                        ),
                    ));
                };
                self.advance();
                repr.push('\n');
                repr.push_str(fline);
            } else {
                return Err(unexpected(
                    flineno,
                    format!("expected 'field', got {ftrimmed:?}"),
                ));
            }
        }
        raw.into_iter().map(|f| self.finish_field(f)).collect()
    }

    fn parse_field_line(&self, line: usize, fbody: &str) -> Result<RawField, SnapshotError> {
        let (name, tail) = match fbody.split_once(' ') {
            Some((name, tail)) => (name, Some(tail)),
            None => (fbody, None),
        };
        let name = FieldName::from_str(name).map_err(SnapshotError::Name)?;
        let (type_text, default_text) = match tail {
            None => (None, None),
            Some(tail) => match (tail.strip_prefix(":: "), tail.strip_prefix("= ")) {
                (Some(typed), _) => (Some(typed.trim().to_owned()), None),
                (None, Some(default)) => (None, Some(default.to_owned())),
                (None, None) => {
                    return Err(unexpected(
                        line,
                        format!("expected '::' or '=' after field {name}, got {tail:?}"),
                    ));
                }
            },
        };
        Ok(RawField {
            line,
            name,
            type_text,
            default_text,
        })
    }

    fn finish_field(&self, raw: RawField) -> Result<RecordField, SnapshotError> {
        let RawField {
            line,
            name,
            type_text,
            default_text,
        } = raw;
        let split = type_text
            .as_deref()
            .and_then(|t| t.split_once(" = "))
            .map(|(t, d)| (t.to_owned(), d.to_owned()));
        let (type_repr, default_text) = match split {
            Some((t, d)) => (Some(t), Some(d)),
            None => (type_text, default_text),
        };
        let default = match default_text {
            Some(text) => {
                self.require_format_5(line, "record field default")?;
                Some(
                    NormalisedDefault::from_stored(&text)
                        .map_err(|e| unexpected(line, e.to_string()))?,
                )
            }
            None => None,
        };
        Ok(RecordField {
            name,
            type_repr,
            default,
        })
    }

    fn next_entry(&mut self) -> Option<(usize, &'a str)> {
        let (lineno, line) = self.peek()?;
        let trimmed = line.strip_prefix("  ")?;
        self.advance();
        Some((lineno, trimmed))
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

struct RawField {
    line: usize,
    name: FieldName,
    type_text: Option<String>,
    default_text: Option<String>,
}

struct CuttlefishEntries {
    mappings: BTreeSet<ConfKey>,
    validators: BTreeSet<ValidatorName>,
}

fn unexpected(line: usize, detail: String) -> SnapshotError {
    SnapshotError::UnexpectedToken { line, detail }
}

fn not_canonical(line: usize, detail: String) -> SnapshotError {
    SnapshotError::NotCanonical { line, detail }
}

fn missing_app(line: usize, noun: &str, name: &impl Display) -> SnapshotError {
    unexpected(line, format!("{noun} {name} has no app line"))
}

fn missing_line(line: usize, noun: &str, name: &impl Display, entry: &str) -> SnapshotError {
    unexpected(line, format!("{noun} {name} has no {entry} line"))
}

fn require_header_format_5(
    format_version: u32,
    line: usize,
    key: &str,
) -> Result<(), SnapshotError> {
    if format_version >= 5 {
        return Ok(());
    }
    Err(SnapshotError::MalformedHeader {
        line,
        detail: format!(
            "{key} needs format-version 5, this file is format-version {format_version}"
        ),
    })
}

fn push_in_order<T: Ord + Display>(
    list: &mut Vec<T>,
    item: T,
    line: usize,
    label: &str,
) -> Result<(), SnapshotError> {
    if breaks_order(list.last(), &item) {
        return Err(not_canonical(line, format!("{label} {item} out of order")));
    }
    list.push(item);
    Ok(())
}

fn parse_label<T: FromStr<Err = UnknownLabel>>(
    line: usize,
    text: &str,
) -> Result<T, SnapshotError> {
    text.trim()
        .parse()
        .map_err(|e: UnknownLabel| unexpected(line, e.to_string()))
}

fn parse_unreadable(line: usize, rest: &str) -> Result<ManifestUnreadable, SnapshotError> {
    let (kind, argument) = match rest.trim().split_once(' ') {
        Some((kind, argument)) => (kind, Some(argument.trim())),
        None => (rest.trim(), None),
    };
    match (kind, argument) {
        ("missing", None) => Ok(ManifestUnreadable::Missing),
        ("parse_failed", None) => Ok(ManifestUnreadable::ParseFailed),
        ("unexpanded_variable", Some(name)) => Ok(ManifestUnreadable::UnexpandedVariable(
            MakeVariableName::from_str(name).map_err(SnapshotError::Name)?,
        )),
        _ => Err(unexpected(
            line,
            format!("unknown requires_unreadable reason {rest:?}"),
        )),
    }
}

fn parse_header_ref(line: usize, rest: &str) -> Result<HeaderRef, SnapshotError> {
    let (app, app_path) = rest
        .trim()
        .split_once(' ')
        .ok_or_else(|| unexpected(line, format!("expected 'app path' in include {rest:?}")))?;
    Ok(HeaderRef {
        app: ApplicationName::from_str(app).map_err(SnapshotError::Name)?,
        app_path: RelativePath::from_str(app_path.trim()).map_err(SnapshotError::Name)?,
    })
}

fn parse_define_line(line: usize, rest: &str) -> Result<MacroDecl, SnapshotError> {
    let (key, body) = rest
        .split_once(' ')
        .ok_or_else(|| unexpected(line, format!("expected 'NAME body' in define {rest:?}")))?;
    let key = MacroKey::from_str(key).map_err(SnapshotError::Name)?;
    let body = if body == "?" {
        MacroBody::Conditional
    } else {
        MacroBody::Text(
            NormalisedMacroBody::from_stored(body).map_err(|e| unexpected(line, e.to_string()))?,
        )
    };
    Ok(MacroDecl { key, body })
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
        .ok_or_else(|| format!("expected 'name/arity body' in {s:?}"))?;
    let fa = parse_fun_arity(head).map_err(|e| e.to_string())?;
    Ok((fa, sig.to_owned()))
}

fn parse_test_only_export(s: &str) -> Result<TestOnlyExport, String> {
    let mut parts = s.split_whitespace();
    let variant_token = parts
        .next()
        .ok_or_else(|| format!("missing variant in test_only_export {s:?}"))?;
    let variant = TestExportVariant::from_label(variant_token)
        .ok_or_else(|| format!("unknown test_only_export variant {variant_token:?}"))?;
    let fa_token = parts
        .next()
        .ok_or_else(|| format!("missing function/arity in test_only_export {s:?}"))?;
    let fa = parse_fun_arity(fa_token).map_err(|e| e.to_string())?;
    let mut export_line: Option<usize> = None;
    let mut body_line: Option<usize> = None;
    for token in parts {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("expected key=value in test_only_export, got {token:?}"))?;
        match key {
            "export_line" => {
                export_line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid export_line: {e}"))?,
                );
            }
            "body_line" => {
                body_line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid body_line: {e}"))?,
                );
            }
            other => return Err(format!("unknown test_only_export key {other:?}")),
        }
    }
    let export_line = export_line.ok_or_else(|| "missing export_line".to_owned())?;
    if matches!(variant, TestExportVariant::B) && body_line.is_none() {
        return Err("variant=b requires body_line".into());
    }
    Ok(TestOnlyExport {
        function: fa.name,
        arity: fa.arity,
        export_line,
        body_line,
        variant,
    })
}

fn parse_ifdef_macro(s: &str) -> Result<IfdefMacro, String> {
    let mut parts = s.split_whitespace();
    let name_token = parts
        .next()
        .ok_or_else(|| format!("missing macro name in ifdef_macro {s:?}"))?;
    let name = name_token.to_owned();
    let mut guard_kind: Option<IfdefGuardKind> = None;
    let mut line: Option<usize> = None;
    for token in parts {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("expected key=value in ifdef_macro, got {token:?}"))?;
        match key {
            "guard" => {
                guard_kind = Some(
                    IfdefGuardKind::from_label(value)
                        .ok_or_else(|| format!("unknown ifdef_macro guard {value:?}"))?,
                );
            }
            "line" => {
                line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid line: {e}"))?,
                );
            }
            other => return Err(format!("unknown ifdef_macro key {other:?}")),
        }
    }
    let guard_kind = guard_kind.ok_or_else(|| "missing guard".to_owned())?;
    let line = line.ok_or_else(|| "missing line".to_owned())?;
    Ok(IfdefMacro {
        name,
        line,
        guard_kind,
    })
}

fn parse_variant_c_block(s: &str) -> Result<VariantCBlock, String> {
    let mut guard: Option<String> = None;
    let mut start_line: Option<usize> = None;
    let mut else_line: Option<usize> = None;
    let mut end_line: Option<usize> = None;
    for token in s.split_whitespace() {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("expected key=value in variant_c_block, got {token:?}"))?;
        match key {
            "guard" => guard = Some(value.to_owned()),
            "start_line" => {
                start_line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid start_line: {e}"))?,
                );
            }
            "else_line" => {
                else_line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid else_line: {e}"))?,
                );
            }
            "end_line" => {
                end_line = Some(
                    value
                        .parse()
                        .map_err(|e: ParseIntError| format!("invalid end_line: {e}"))?,
                );
            }
            other => return Err(format!("unknown variant_c_block key {other:?}")),
        }
    }
    let guard = guard.ok_or_else(|| "missing guard".to_owned())?;
    let start_line = start_line.ok_or_else(|| "missing start_line".to_owned())?;
    let end_line = end_line.ok_or_else(|| "missing end_line".to_owned())?;
    Ok(VariantCBlock {
        guard,
        start_line,
        end_line,
        else_line,
    })
}

fn parse_type_decl_line(s: &str) -> Result<(TypeArity, String), String> {
    let (head, rhs) = s
        .split_once(" :: ")
        .ok_or_else(|| format!("expected 'name/arity :: rhs' in {s:?}"))?;
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
            .ok_or_else(|| format!("expected name/arity got {head:?}"))?;
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
                    .ok_or_else(|| format!("expected use name/arity got {target:?}"))?;
                replacement = Some(DeprecationReplacement {
                    function: FunctionName::from_str(n).map_err(|e| e.to_string())?,
                    arity: Arity::from_str(a).map_err(|e| e.to_string())?,
                });
            }
            "reason" => {
                // decode the reason from its first to last quote, preserving internal spacing and escapes
                let open = rest.find('"').ok_or("reason must be a quoted string")?;
                let close = rest.rfind('"').ok_or("reason must be a quoted string")?;
                if close <= open {
                    return Err("reason must be a quoted string".into());
                }
                reason = Some(unescape_reason(&rest[open + 1..close]));
                break;
            }
            other => return Err(format!("unexpected deprecation token {other:?}")),
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

// reverse the writer's Rust-debug quoting; unknown escapes pass through so no bytes are lost
fn unescape_reason(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A `name/arity` entry with a canonical order, so one `check_order`
/// gates both the function-arity and type-arity streams.
trait ArityOrdered {
    type Key: Ord;
    fn order_key(&self) -> Self::Key;
    fn describe(&self) -> String;
}

impl ArityOrdered for FunArity {
    type Key = (FunctionName, Arity);
    fn order_key(&self) -> Self::Key {
        (self.name.clone(), self.arity)
    }
    fn describe(&self) -> String {
        format!("{}/{}", self.name, self.arity)
    }
}

impl ArityOrdered for TypeArity {
    type Key = (TypeName, Arity);
    fn order_key(&self) -> Self::Key {
        (self.name.clone(), self.arity)
    }
    fn describe(&self) -> String {
        format!("{}/{}", self.name, self.arity)
    }
}

fn check_order<T: ArityOrdered>(
    existing: &[T],
    item: &T,
    lineno: usize,
    label: &str,
) -> Result<(), SnapshotError> {
    if let Some(prev) = existing.last() {
        if item.order_key() <= prev.order_key() {
            return Err(SnapshotError::NotCanonical {
                line: lineno,
                detail: format!("{label} {} out of order", item.describe()),
            });
        }
    }
    Ok(())
}

fn parse_dep_pin_line(line: usize, value: &str) -> Result<VendoredDep, SnapshotError> {
    let (name_part, after_eq) =
        value
            .split_once('=')
            .ok_or_else(|| SnapshotError::MalformedHeader {
                line,
                detail: format!("dep-pin missing '=': {value:?}"),
            })?;
    let name_str = name_part.trim();
    let source_and_version = after_eq.trim();
    let (source_label, version_str) =
        source_and_version
            .split_once(' ')
            .ok_or_else(|| SnapshotError::MalformedHeader {
                line,
                detail: format!("dep-pin missing version after source: {value:?}"),
            })?;
    let source_label = source_label.trim();
    let source = VendoredDepSource::from_label(source_label).ok_or_else(|| {
        SnapshotError::MalformedHeader {
            line,
            detail: format!("unknown dep-pin source {source_label:?}"),
        }
    })?;
    let name = DependencyName::from_str(name_str).map_err(SnapshotError::Name)?;
    let version = DependencyVersion::from_str(version_str.trim()).map_err(SnapshotError::Name)?;
    Ok(VendoredDep {
        name,
        version,
        source,
    })
}

fn parse_versioned_machine(rest: &str) -> Result<VersionedMachineVersion, String> {
    let (fa_part, after_fa) = rest
        .split_once('=')
        .ok_or_else(|| format!("versioned_machine missing '=': {rest:?}"))?;
    let fa = parse_fun_arity(fa_part.trim()).map_err(|e| e.to_string())?;
    let after_fa = after_fa.trim();
    let (value_part, provenance_part) = match after_fa.split_once(' ') {
        Some((v, p)) => (v.trim(), p.trim()),
        None => (after_fa, ""),
    };
    let value = match value_part {
        "-" => None,
        s => Some(
            s.parse::<u64>()
                .map_err(|e| format!("versioned_machine value: {e}"))?,
        ),
    };
    let provenance = parse_provenance(provenance_part)?;
    Ok(VersionedMachineVersion {
        function: fa.name,
        arity: fa.arity,
        value,
        provenance,
    })
}

fn parse_provenance(rest: &str) -> Result<Provenance, String> {
    if rest.is_empty() || rest == "literal" {
        return Ok(Provenance::Literal);
    }
    if let Some(after) = rest.strip_prefix("macro_body ") {
        let (name_str, at_tail) = match after.split_once(" at ") {
            Some((n, p)) => (n.trim(), Some(p.trim().to_owned())),
            None => (after.trim(), None),
        };
        let macro_name = MacroName::from_str(name_str).map_err(|e| e.to_string())?;
        return Ok(Provenance::MacroBody {
            macro_name,
            defined_in: at_tail,
        });
    }
    Err(format!("unknown provenance {rest:?}"))
}

fn parse_wire_constant(rest: &str) -> Result<WireConstantBinding, String> {
    let (name_part, after_eq) = rest
        .split_once('=')
        .ok_or_else(|| format!("wire_constant missing '=': {rest:?}"))?;
    let macro_name = MacroName::from_str(name_part.trim()).map_err(|e| e.to_string())?;
    let body = after_eq.trim();
    let (body_no_at, defined_in) = match body.rsplit_once(" at ") {
        Some((b, p)) => (b.trim(), Some(p.trim().to_owned())),
        None => (body, None),
    };
    let (kind, value_str) = body_no_at
        .split_once(' ')
        .ok_or_else(|| format!("wire_constant missing type/value: {rest:?}"))?;
    let value = match kind.trim() {
        "u64" => WireValue::U64(
            value_str
                .trim()
                .parse::<u64>()
                .map_err(|e| format!("wire_constant u64 value: {e}"))?,
        ),
        "bytes" => WireValue::Bytes(value_str.trim().as_bytes().to_vec()),
        "opaque" => WireValue::Opaque(value_str.trim().to_owned()),
        other => return Err(format!("unknown wire_constant kind {other:?}")),
    };
    Ok(WireConstantBinding {
        macro_name,
        value,
        defined_in,
    })
}

#[derive(Debug)]
struct ClassOrder<T> {
    label: &'static str,
    last: Option<T>,
}

impl<T: Copy + Ord + Debug> ClassOrder<T> {
    fn new(label: &'static str) -> Self {
        Self { label, last: None }
    }

    fn advance(&mut self, c: T, line: usize) -> Result<(), SnapshotError> {
        if let Some(prev) = self.last
            && c < prev
        {
            return Err(SnapshotError::NotCanonical {
                line,
                detail: format!("{}: {c:?} after {prev:?}", self.label),
            });
        }
        self.last = Some(c);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EntryClass {
    Visibility,
    Path,
    App,
    Behaviour,
    Export,
    ExportType,
    Callback,
    OptionalCallback,
    Spec,
    Type,
    Opaque,
    Deprecated,
    Record,
    TestOnlyExport,
    IfdefMacro,
    VariantCBlock,
    VersionedMachine,
    WireConstant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HrlEntryClass {
    App,
    Include,
    Type,
    Opaque,
    Record,
    Define,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SectionClass {
    App,
    Module,
    Header,
    Schema,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AppEntryClass {
    Manifest,
    Requires,
    RequiresUnreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CuttlefishEntryClass {
    App,
    Name,
    Mapping,
    Validator,
}
