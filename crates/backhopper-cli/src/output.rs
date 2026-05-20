// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Formatter dispatch for the CLI: a single envelope shape for JSON,
//! a closure for text. The JSON body always carries `schema_version`,
//! `command`, `data`, and `exit_code` so clients can parse without
//! conditionals.

use std::io::{self, Write};

use serde::Serialize;
use serde_json::json;

use crate::cli::Formatter;
use crate::errors::{CliError, CliResult};

#[derive(Debug)]
pub struct OutputContext {
    pub formatter: Formatter,
    pub command: &'static str,
    pub schema_version: u32,
}

impl OutputContext {
    pub fn new(formatter: Formatter, command: &'static str) -> Self {
        Self {
            formatter,
            command,
            schema_version: 1,
        }
    }
}

pub fn render<T, FT>(out: &OutputContext, payload: &T, text_render: FT) -> CliResult<()>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    render_with_exit(out, payload, 0, text_render).map(|_| ())
}

pub fn render_with_exit<T, FT>(
    out: &OutputContext,
    payload: &T,
    exit_code: i32,
    text_render: FT,
) -> CliResult<i32>
where
    T: Serialize,
    FT: FnOnce(&mut dyn Write) -> CliResult<()>,
{
    let mut stdout = io::stdout().lock();
    match out.formatter {
        Formatter::Json => {
            let body = json!({
                "schema_version": out.schema_version,
                "command":        out.command,
                "data":           payload,
                "exit_code":      exit_code,
            });
            serde_json::to_writer_pretty(&mut stdout, &body)
                .map_err(|e| CliError::OutputError(e.to_string()))?;
            writeln!(stdout)?;
        }
        Formatter::Text => {
            text_render(&mut stdout)?;
        }
    }
    Ok(exit_code)
}