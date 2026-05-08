//! Canonical writer for the snapshot file format.
//!
//! Only `Snapshot<state::Canonical>` is writable; the type system ensures
//! a non-canonical snapshot can never reach this code.

use std::io::{self, Write};

use time::format_description::well_known::Rfc3339;

use crate::errors::SnapshotError;
use crate::model::snapshot::{
    ArityMatch, Deprecation, FORMAT_VERSION, HrlFile, Module, Snapshot, SnapshotHeader, Visibility,
    state,
};

pub const HEADER_PREFIX: &str = "# ";

pub fn write<W: Write>(
    snapshot: &Snapshot<state::Canonical>,
    w: &mut W,
) -> Result<(), SnapshotError> {
    write_header(snapshot.header(), w)?;
    let mut first = true;
    for module in snapshot.modules() {
        if !first {
            writeln!(w)?;
        }
        first = false;
        write_module(module, w)?;
    }
    for hrl in snapshot.headers() {
        if !first {
            writeln!(w)?;
        }
        first = false;
        write_hrl(hrl, w)?;
    }
    Ok(())
}

pub fn to_string(snapshot: &Snapshot<state::Canonical>) -> Result<String, SnapshotError> {
    let mut buf = Vec::new();
    write(snapshot, &mut buf)?;
    String::from_utf8(buf).map_err(|e| SnapshotError::InvalidUtf8 {
        offset: e.utf8_error().valid_up_to(),
    })
}

fn write_header<W: Write>(header: &SnapshotHeader, w: &mut W) -> io::Result<()> {
    writeln!(w, "# backhopper snapshot")?;
    writeln!(w, "# format-version: {}", FORMAT_VERSION)?;
    writeln!(w, "# project: {}", header.project)?;
    writeln!(w, "# tag: {}", header.tag)?;
    if let Some(branch) = &header.branch {
        writeln!(w, "# branch: {}", branch)?;
    }
    writeln!(w, "# commit: {}", header.commit)?;
    writeln!(w, "# scanned-paths: {}", header.scanned_paths.join(", "))?;
    writeln!(w, "# generated-by: {}", header.generated_by)?;
    let formatted = header
        .generated_at
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::from("0"));
    writeln!(w, "# generated-at: {}", formatted)?;
    writeln!(w)?;
    Ok(())
}

fn write_module<W: Write>(m: &Module, w: &mut W) -> io::Result<()> {
    writeln!(w, "module {}", m.name)?;
    if m.visibility != Visibility::Public {
        writeln!(w, "  visibility {}", m.visibility.keyword())?;
    }
    for b in &m.behaviours {
        writeln!(w, "  behaviour {}", b)?;
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
    Ok(())
}

fn write_hrl<W: Write>(h: &HrlFile, w: &mut W) -> io::Result<()> {
    writeln!(w, "header {}", h.path)?;
    for t in &h.types {
        let body = format!("{}/{} :: {}", t.name, t.arity, t.rhs);
        write_keyword_signature(w, "type", &body, "")?;
    }
    for o in &h.opaques {
        writeln!(w, "  opaque {}/{}", o.name, o.arity)?;
    }
    for r in &h.records {
        writeln!(w, "  record {}", r.name)?;
        for f in &r.fields {
            if let Some(t) = &f.type_repr {
                writeln!(w, "    field {} :: {}", f.name, t)?;
            } else {
                writeln!(w, "    field {}", f.name)?;
            }
        }
    }
    Ok(())
}

fn write_keyword_signature<W: Write>(
    w: &mut W,
    keyword: &str,
    head: &str,
    body: &str,
) -> io::Result<()> {
    let combined = if body.is_empty() {
        head.to_owned()
    } else {
        format!("{} {}", head, body)
    };
    let mut iter = combined.split('\n');
    if let Some(first) = iter.next() {
        writeln!(w, "  {} {}", keyword, first)?;
    }
    for cont in iter {
        writeln!(w, "{}", cont)?;
    }
    Ok(())
}

fn write_deprecation<W: Write>(d: &Deprecation, w: &mut W) -> io::Result<()> {
    let head = match (&d.function, &d.arity_match, d.module_wide) {
        (_, _, true) => String::from("deprecated module"),
        (Some(f), ArityMatch::Exact { arity }, _) => format!("deprecated {}/{}", f, arity),
        (Some(f), ArityMatch::Any, _) => format!("deprecated {}/*", f),
        (None, _, _) => String::from("deprecated *"),
    };
    let mut tail = String::new();
    if let Some(since) = &d.since {
        tail.push_str(&format!(" since {}", since));
    }
    if let Some(rep) = &d.replacement {
        tail.push_str(&format!(" use {}/{}", rep.function, rep.arity));
    }
    if let Some(reason) = &d.reason {
        tail.push_str(&format!(" reason {:?}", reason));
    }
    writeln!(w, "  {}{}", head, tail)?;
    Ok(())
}
