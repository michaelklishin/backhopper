// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::BTreeSet;
use std::fmt::Write;
use std::path::PathBuf;

use backhopper_core::jigsaw::surface::ReleaseSurface;
use backhopper_core::model::macro_decl::MacroBody;
use backhopper_core::model::names::ApplicationName;
use backhopper_core::model::snapshot::HeaderRef;

/// A host header the build patch records, rendered as an `.hrl` the call
/// reader can read: its own includes first, so the reader chains through
/// them, then one `-define` per macro it defines. A macro defined under a
/// conditional is left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntheticHeader {
    pub path: PathBuf,
    pub text: String,
}

const EMPTY_BODY: &str = "%empty%";

/// Placed at `<app>/<app_path>`, where `-include_lib` and the reader's
/// header index look for it.
pub(crate) fn render(
    build: &ReleaseSurface<'_>,
    roots: &BTreeSet<HeaderRef>,
    plugin_apps: &BTreeSet<&ApplicationName>,
) -> Vec<SyntheticHeader> {
    build
        .header_closure(roots)
        .into_iter()
        .filter(|(reference, _)| !plugin_apps.contains(&reference.app))
        .map(|(reference, header)| {
            let mut text = String::new();
            for include in &header.includes {
                let _ = writeln!(text, "-include_lib(\"{include}\").");
            }
            for decl in &header.defines {
                let MacroBody::Text(body) = &decl.body else {
                    continue;
                };
                let body = if body.as_str() == EMPTY_BODY {
                    ""
                } else {
                    body.as_str()
                };
                let name = &decl.key.name;
                match decl.key.arity {
                    Some(arity) => {
                        let params: Vec<String> = (1..=u32::from(arity.get()))
                            .map(|n| format!("A{n}"))
                            .collect();
                        let _ = writeln!(text, "-define({name}({}), {body}).", params.join(", "));
                    }
                    None => {
                        let _ = writeln!(text, "-define({name}, {body}).");
                    }
                }
            }
            SyntheticHeader {
                path: PathBuf::from(reference.to_string()),
                text,
            }
        })
        .collect()
}
