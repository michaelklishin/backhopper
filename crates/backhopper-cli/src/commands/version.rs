// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use clap::crate_version;
use serde::Serialize;

use crate::cli::GlobalArgs;
use crate::errors::CliResult;
use crate::output::{OutputContext, render};

#[derive(Debug, Serialize)]
struct VersionPayload<'a> {
    name: &'a str,
    version: &'a str,
}

pub fn handle(args: &GlobalArgs) -> CliResult<i32> {
    let payload = VersionPayload {
        name: "backhopper",
        version: crate_version!(),
    };
    let ctx = OutputContext::new(args.formatter, "version");
    render(&ctx, &payload, |w| {
        writeln!(w, "{} {}", payload.name, payload.version)?;
        Ok(())
    })?;
    Ok(0)
}
