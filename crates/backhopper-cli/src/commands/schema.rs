// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use backhopper_core::schema::{
    CURRENT_SCHEMA_VERSION, SchemaError, embedded_versions, schema_value_for,
};
use backhopper_core::schema_diff::{SchemaDiff, diff};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{GlobalArgs, SchemaCmd, SchemaDiffArgs, SchemaShowArgs};
use crate::errors::{CliError, CliResult};
use crate::output::{OutputContext, render};

pub fn handle(args: &GlobalArgs, cmd: SchemaCmd) -> CliResult<i32> {
    match cmd {
        SchemaCmd::Show(a) => show(args, a),
        SchemaCmd::Diff(a) => diff_cmd(args, a),
        SchemaCmd::SupportedEnvelopeVersions => supported_envelope_versions(args),
    }
}

fn supported_envelope_versions(args: &GlobalArgs) -> CliResult<i32> {
    let payload = SupportedEnvelopeVersionsPayload {
        current_envelope_version: CURRENT_SCHEMA_VERSION,
        supported_envelope_versions: embedded_versions(),
        backhopper_version: env!("CARGO_PKG_VERSION").into(),
    };
    let ctx = OutputContext::new(args.formatter, "schema supported_envelope_versions");
    render(&ctx, &payload, |w| {
        writeln!(
            w,
            "current_envelope_version: {}",
            payload.current_envelope_version
        )?;
        writeln!(
            w,
            "supported_envelope_versions: [{}]",
            payload
                .supported_envelope_versions
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        )?;
        writeln!(w, "backhopper_version: {}", payload.backhopper_version)?;
        Ok(())
    })?;
    Ok(0)
}

fn show(args: &GlobalArgs, a: SchemaShowArgs) -> CliResult<i32> {
    let value = schema_value_for(a.version).map_err(map_schema_err)?;
    let ctx = OutputContext::new(args.formatter, "schema show");
    let payload = ShowPayload {
        version: a.version,
        schema: value,
    };
    render(&ctx, &payload, |w| {
        serde_json::to_writer_pretty(&mut *w, &payload.schema)
            .map_err(|e| CliError::OutputError(e.to_string()))?;
        writeln!(w)?;
        Ok(())
    })?;
    Ok(0)
}

fn diff_cmd(args: &GlobalArgs, a: SchemaDiffArgs) -> CliResult<i32> {
    let from = schema_value_for(a.from).map_err(map_schema_err)?;
    let to = schema_value_for(a.to).map_err(map_schema_err)?;
    let result: SchemaDiff = diff(a.from, a.to, &from, &to);
    let ctx = OutputContext::new(args.formatter, "schema diff");
    render(&ctx, &result, |w| {
        writeln!(w, "from v{} to v{}", result.from, result.to)?;
        if result.added_paths.is_empty()
            && result.removed_paths.is_empty()
            && result.changed_types.is_empty()
        {
            writeln!(w, "  (no differences)")?;
            return Ok(());
        }
        for p in &result.added_paths {
            writeln!(w, "  added:   {p}")?;
        }
        for p in &result.removed_paths {
            writeln!(w, "  removed: {p}")?;
        }
        for c in &result.changed_types {
            writeln!(w, "  changed: {}  {} -> {}", c.path, c.old_type, c.new_type)?;
        }
        Ok(())
    })?;
    Ok(0)
}

fn map_schema_err(e: SchemaError) -> CliError {
    let SchemaError::UnknownVersion { requested, known } = e;
    CliError::InvalidInput(format!(
        "no schema embedded for version {requested}; known: {known:?}"
    ))
}

#[derive(Debug, Serialize)]
struct ShowPayload {
    version: u32,
    schema: Value,
}

#[derive(Debug, Serialize)]
struct SupportedEnvelopeVersionsPayload {
    current_envelope_version: u32,
    supported_envelope_versions: Vec<u32>,
    backhopper_version: String,
}
