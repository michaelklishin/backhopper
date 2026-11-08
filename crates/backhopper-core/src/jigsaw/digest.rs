// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use crate::config::PinSource;
use crate::digest::DigestWriter;
use crate::model::names::HostFactsDigest;
use crate::model::release_roots::{MakefileVariable, ReleaseRoots};

#[must_use]
pub fn host_facts_digest(pins: &PinSource, roots: &ReleaseRoots) -> HostFactsDigest {
    let mut writer = DigestWriter::new("host facts");
    feed_pins(&mut writer, pins);
    feed_roots(&mut writer, roots);
    HostFactsDigest::from_digest(writer.finish())
}

fn feed_pins(writer: &mut DigestWriter, pins: &PinSource) {
    match pins {
        PinSource::ErlangMk { file_path } => {
            writer.text("erlang_mk").text(file_path.as_str());
        }
        PinSource::Table(table) => {
            writer.text("table").count(table.len());
            for (release, deps) in table {
                writer.text(&release.to_string()).count(deps.len());
                for (name, version) in deps {
                    writer.text(name.as_str()).text(version.as_str());
                }
            }
        }
    }
}

fn feed_roots(writer: &mut DigestWriter, roots: &ReleaseRoots) {
    match roots {
        ReleaseRoots::Apps(apps) => {
            writer.text("apps");
            feed_apps(writer, apps.iter().map(|app| app.as_str()));
        }
        ReleaseRoots::Variable(variable) => {
            writer.text("variable");
            feed_variable(writer, variable);
        }
        ReleaseRoots::Both { apps, variable } => {
            writer.text("both");
            feed_apps(writer, apps.iter().map(|app| app.as_str()));
            feed_variable(writer, variable);
        }
    }
}

fn feed_apps<'a>(writer: &mut DigestWriter, apps: impl Iterator<Item = &'a str>) {
    let mut apps: Vec<&str> = apps.collect();
    apps.sort_unstable();
    apps.dedup();
    writer.count(apps.len());
    for app in apps {
        writer.text(app);
    }
}

fn feed_variable(writer: &mut DigestWriter, variable: &MakefileVariable) {
    writer
        .text(variable.file_path.as_str())
        .text(variable.variable.as_str());
}
