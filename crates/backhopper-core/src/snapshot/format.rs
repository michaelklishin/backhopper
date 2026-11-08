// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Canonical writer for the snapshot file format.
//!
//! Only `Snapshot<state::Canonical>` is writable; the type system ensures
//! a non-canonical snapshot can never reach this code.

use std::io::{self, Write};

use time::format_description::well_known::Rfc3339;

use crate::errors::SnapshotError;
use crate::model::app_manifest::{AppManifest, ManifestDeps, ManifestUnreadable};
use crate::model::macro_decl::{MacroBody, MacroDecl};
use crate::model::names::{ConfKey, ModuleName, ValidatorName};
use crate::model::schema_files::{PartialFile, SchemaFile};
use crate::model::snapshot::{
    ArityMatch, Deprecation, HrlFile, Module, Provenance, RecordDecl, Snapshot, SnapshotHeader,
    VendoredDep, VersionedMachineVersion, Visibility, WireConstantBinding, WireValue, state,
};

pub const HEADER_PREFIX: &str = "# ";

fn write_section<T, W: Write>(
    w: &mut W,
    first: &mut bool,
    items: &[T],
    mut write_item: impl FnMut(&T, &mut W) -> io::Result<()>,
) -> io::Result<()> {
    for item in items {
        if !*first {
            writeln!(w)?;
        }
        *first = false;
        write_item(item, w)?;
    }
    Ok(())
}

pub fn write<W: Write>(
    snapshot: &Snapshot<state::Canonical>,
    w: &mut W,
) -> Result<(), SnapshotError> {
    if let Some(detail) = snapshot.unwritable_reason() {
        return Err(SnapshotError::Unwritable { detail });
    }
    write_header(snapshot.header(), snapshot.written_format_version(), w)?;
    let mut first = true;
    write_section(w, &mut first, snapshot.apps(), write_app)?;
    write_section(w, &mut first, snapshot.modules(), write_module)?;
    write_section(w, &mut first, snapshot.headers(), write_hrl)?;
    write_section(w, &mut first, snapshot.schemas(), write_schema)?;
    write_section(w, &mut first, snapshot.partials(), write_partial)?;
    Ok(())
}

pub fn to_string(snapshot: &Snapshot<state::Canonical>) -> Result<String, SnapshotError> {
    let mut buf = Vec::new();
    write(snapshot, &mut buf)?;
    String::from_utf8(buf).map_err(|e| SnapshotError::InvalidUtf8 {
        offset: e.utf8_error().valid_up_to(),
    })
}

/// Write the canonical header followed by the single named module's section.
/// Returns `Ok(false)` if no such module is present in the snapshot.
pub fn write_module_filtered<W: Write>(
    snapshot: &Snapshot<state::Canonical>,
    module: &ModuleName,
    w: &mut W,
) -> Result<bool, SnapshotError> {
    write_header(snapshot.header(), snapshot.written_format_version(), w)?;
    let Some(m) = snapshot.module_named(module) else {
        return Ok(false);
    };
    write_module(m, w)?;
    Ok(true)
}

fn write_header<W: Write>(
    header: &SnapshotHeader,
    format_version: u32,
    w: &mut W,
) -> io::Result<()> {
    writeln!(w, "# backhopper snapshot")?;
    writeln!(w, "# format-version: {format_version}")?;
    writeln!(w, "# project: {}", header.project)?;
    writeln!(w, "# tag: {}", header.tag)?;
    if let Some(branch) = &header.branch {
        writeln!(w, "# branch: {branch}")?;
    }
    writeln!(w, "# commit: {}", header.commit)?;
    writeln!(w, "# scanned-paths: {}", header.scanned_paths.join(", "))?;
    if !header.apps_scanned.is_empty() {
        let names: Vec<&str> = header.apps_scanned.iter().map(|a| a.as_str()).collect();
        writeln!(w, "# apps-scanned: {}", names.join(", "))?;
    }
    writeln!(w, "# generated-by: {}", header.generated_by)?;
    let formatted = header
        .generated_at
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::from("0"));
    writeln!(w, "# generated-at: {formatted}")?;
    if !header.extractor_version.is_empty() {
        writeln!(w, "# extractor-version: {}", header.extractor_version)?;
    }
    if let Some(digest) = &header.extraction_settings {
        writeln!(w, "# extraction-settings: {digest}")?;
    }
    for pin in &header.dep_pins {
        write_dep_pin(w, pin)?;
    }
    if let Some(host) = &header.host_facts {
        for root in &host.release_roots {
            writeln!(w, "# release-root: {root}")?;
        }
        writeln!(w, "# host-facts: {}", host.digest)?;
    }
    writeln!(w)?;
    Ok(())
}

fn write_dep_pin<W: Write>(w: &mut W, pin: &VendoredDep) -> io::Result<()> {
    let label = pin.source.as_label();
    writeln!(w, "# dep-pin: {} = {} {}", pin.name, label, pin.version)
}

/// One module's canonical section text, without any snapshot header:
/// the shape `tree show` prints for a live tree, where no snapshot
/// exists to head it.
pub fn module_to_string(m: &Module) -> Result<String, SnapshotError> {
    let mut buf = Vec::new();
    write_module(m, &mut buf)?;
    String::from_utf8(buf).map_err(|e| SnapshotError::InvalidUtf8 {
        offset: e.utf8_error().valid_up_to(),
    })
}

fn write_module<W: Write>(m: &Module, w: &mut W) -> io::Result<()> {
    writeln!(w, "module {}", m.name)?;
    if m.visibility != Visibility::Public {
        writeln!(w, "  visibility {}", m.visibility.keyword())?;
    }
    if let Some(path) = &m.path {
        writeln!(w, "  path {path}")?;
    }
    if let Some(app) = &m.app {
        writeln!(w, "  app {app}")?;
    }
    for b in &m.behaviours {
        writeln!(w, "  behaviour {b}")?;
    }
    for e in &m.exports {
        writeln!(w, "  export {}/{}", e.name, e.arity)?;
    }
    for et in &m.export_types {
        writeln!(w, "  export_type {}/{}", et.name, et.arity)?;
    }
    for c in &m.callbacks {
        write_keyword_signature(
            w,
            "callback",
            &format!("{}/{}", c.name, c.arity),
            &c.signature,
        )?;
    }
    for oc in &m.optional_callbacks {
        writeln!(w, "  optional_callback {}/{}", oc.name, oc.arity)?;
    }
    for s in &m.specs {
        write_keyword_signature(w, "spec", &format!("{}/{}", s.name, s.arity), &s.signature)?;
    }
    for t in &m.types {
        let body = format!("{}/{} :: {}", t.name, t.arity, t.rhs);
        write_keyword_signature(w, "type", &body, "")?;
    }
    for o in &m.opaques {
        writeln!(w, "  opaque {}/{}", o.name, o.arity)?;
    }
    for d in &m.deprecations {
        write_deprecation(d, w)?;
    }
    for r in &m.records {
        write_record(r, w)?;
    }
    for t in &m.test_only_exports {
        let variant = t.variant.as_label();
        match t.body_line {
            Some(body) => writeln!(
                w,
                "  test_only_export {} {}/{} export_line={} body_line={}",
                variant, t.function, t.arity, t.export_line, body
            )?,
            None => writeln!(
                w,
                "  test_only_export {} {}/{} export_line={}",
                variant, t.function, t.arity, t.export_line
            )?,
        }
    }
    for im in &m.ifdef_macros {
        let guard = im.guard_kind.as_label();
        writeln!(
            w,
            "  ifdef_macro {} guard={} line={}",
            im.name, guard, im.line
        )?;
    }
    for vc in &m.variant_c_blocks {
        match vc.else_line {
            Some(else_line) => writeln!(
                w,
                "  variant_c_block guard={} start_line={} else_line={} end_line={}",
                vc.guard, vc.start_line, else_line, vc.end_line
            )?,
            None => writeln!(
                w,
                "  variant_c_block guard={} start_line={} end_line={}",
                vc.guard, vc.start_line, vc.end_line
            )?,
        }
    }
    if let Some(v) = &m.versioned_machine_version {
        write_versioned_machine(w, v)?;
    }
    for wc in &m.wire_constants {
        write_wire_constant(w, wc)?;
    }
    Ok(())
}

fn write_versioned_machine<W: Write>(w: &mut W, v: &VersionedMachineVersion) -> io::Result<()> {
    let value = match v.value {
        Some(n) => n.to_string(),
        None => "-".to_owned(),
    };
    match &v.provenance {
        Provenance::Literal => writeln!(
            w,
            "  versioned_machine {}/{} = {} literal",
            v.function, v.arity, value
        ),
        Provenance::MacroBody {
            macro_name,
            defined_in: Some(path),
        } => writeln!(
            w,
            "  versioned_machine {}/{} = {} macro_body {} at {}",
            v.function, v.arity, value, macro_name, path
        ),
        Provenance::MacroBody {
            macro_name,
            defined_in: None,
        } => writeln!(
            w,
            "  versioned_machine {}/{} = {} macro_body {}",
            v.function, v.arity, value, macro_name
        ),
    }
}

fn write_wire_constant<W: Write>(w: &mut W, wc: &WireConstantBinding) -> io::Result<()> {
    let (kind, body) = match &wc.value {
        WireValue::U64(n) => ("u64", n.to_string()),
        WireValue::Bytes(b) => ("bytes", String::from_utf8_lossy(b).into_owned()),
        WireValue::Opaque(s) => ("opaque", s.clone()),
    };
    match &wc.defined_in {
        Some(path) => writeln!(
            w,
            "  wire_constant {} = {} {} at {}",
            wc.macro_name, kind, body, path
        ),
        None => writeln!(w, "  wire_constant {} = {} {}", wc.macro_name, kind, body),
    }
}

fn write_record<W: Write>(r: &RecordDecl, w: &mut W) -> io::Result<()> {
    writeln!(w, "  record {}", r.name)?;
    for f in &r.fields {
        write!(w, "    field {}", f.name)?;
        if let Some(t) = &f.type_repr {
            write!(w, " :: {t}")?;
        }
        if let Some(d) = &f.default {
            write!(w, " = {d}")?;
        }
        writeln!(w)?;
    }
    Ok(())
}

fn write_hrl<W: Write>(h: &HrlFile, w: &mut W) -> io::Result<()> {
    writeln!(w, "header {}", h.path)?;
    if let Some(app) = &h.app {
        writeln!(w, "  app {app}")?;
    }
    for i in &h.includes {
        writeln!(w, "  include {} {}", i.app, i.app_path)?;
    }
    for t in &h.types {
        let body = format!("{}/{} :: {}", t.name, t.arity, t.rhs);
        write_keyword_signature(w, "type", &body, "")?;
    }
    for o in &h.opaques {
        writeln!(w, "  opaque {}/{}", o.name, o.arity)?;
    }
    for r in &h.records {
        write_record(r, w)?;
    }
    for d in &h.defines {
        write_define(d, w)?;
    }
    Ok(())
}

fn write_define<W: Write>(d: &MacroDecl, w: &mut W) -> io::Result<()> {
    match &d.body {
        MacroBody::Text(body) => writeln!(w, "  define {} {body}", d.key),
        MacroBody::Conditional => writeln!(w, "  define {} ?", d.key),
    }
}

fn write_app<W: Write>(a: &AppManifest, w: &mut W) -> io::Result<()> {
    writeln!(w, "app {}", a.name)?;
    writeln!(w, "  manifest {}", a.source)?;
    match &a.requires {
        ManifestDeps::Listed(apps) => {
            for app in apps {
                writeln!(w, "  requires {app}")?;
            }
            Ok(())
        }
        ManifestDeps::Unreadable(ManifestUnreadable::Missing) => {
            writeln!(w, "  requires_unreadable missing")
        }
        ManifestDeps::Unreadable(ManifestUnreadable::UnexpandedVariable(name)) => {
            writeln!(w, "  requires_unreadable unexpanded_variable {name}")
        }
        ManifestDeps::Unreadable(ManifestUnreadable::ParseFailed) => {
            writeln!(w, "  requires_unreadable parse_failed")
        }
    }
}

fn write_schema<W: Write>(s: &SchemaFile, w: &mut W) -> io::Result<()> {
    writeln!(w, "schema {}", s.path)?;
    writeln!(w, "  app {}", s.app)?;
    write_cuttlefish_keys(w, s.mappings.iter(), s.validators.iter())
}

fn write_partial<W: Write>(p: &PartialFile, w: &mut W) -> io::Result<()> {
    writeln!(w, "partial {}", p.path)?;
    writeln!(w, "  app {}", p.app)?;
    writeln!(w, "  name {}", p.name)?;
    write_cuttlefish_keys(w, p.mappings.iter(), p.validators.iter())
}

fn write_cuttlefish_keys<'a, W: Write>(
    w: &mut W,
    mappings: impl Iterator<Item = &'a ConfKey>,
    validators: impl Iterator<Item = &'a ValidatorName>,
) -> io::Result<()> {
    for m in mappings {
        writeln!(w, "  mapping {m}")?;
    }
    for v in validators {
        writeln!(w, "  validator {v}")?;
    }
    Ok(())
}

fn write_keyword_signature<W: Write>(
    w: &mut W,
    keyword: &str,
    head: &str,
    body: &str,
) -> io::Result<()> {
    if body.is_empty() {
        writeln!(w, "  {keyword} {head}")?;
        return Ok(());
    }
    let mut lines = body.split('\n');
    let first = lines.next().unwrap_or("");
    writeln!(w, "  {keyword} {head} {first}")?;
    for cont in lines {
        writeln!(w, "{cont}")?;
    }
    Ok(())
}

fn write_deprecation<W: Write>(d: &Deprecation, w: &mut W) -> io::Result<()> {
    write!(w, "  ")?;
    match (&d.function, &d.arity_match, d.module_wide) {
        (_, _, true) => write!(w, "deprecated module")?,
        (Some(f), ArityMatch::Exact { arity }, _) => write!(w, "deprecated {f}/{arity}")?,
        (Some(f), ArityMatch::Any, _) => write!(w, "deprecated {f}/*")?,
        (None, _, _) => write!(w, "deprecated *")?,
    }
    if let Some(since) = &d.since {
        write!(w, " since {since}")?;
    }
    if let Some(rep) = &d.replacement {
        write!(w, " use {}/{}", rep.function, rep.arity)?;
    }
    if let Some(reason) = &d.reason {
        write!(w, " reason {reason:?}")?;
    }
    writeln!(w)?;
    Ok(())
}
