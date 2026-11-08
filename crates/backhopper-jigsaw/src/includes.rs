// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::LazyLock;

use backhopper_core::jigsaw::plugin_set::PluginFile;
use backhopper_core::model::names::{ApplicationName, RelativePath};
use backhopper_core::model::snapshot::HeaderRef;
use regex::Regex;

use crate::source::Code;

static INCLUDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*-[ \t]*(include_lib|include)[ \t]*\([ \t]*"([^"\n]*)"[ \t]*\)"#)
        .expect("regex")
});

/// Where an `-include` or `-include_lib` leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// A header of the plugin set.
    Local(PluginFile),
    /// A header of an app the plugin set does not contain.
    Host(HeaderRef),
    /// Neither: the path names nothing in the plugin set and no other app.
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Include {
    pub line: u32,
    pub target: Target,
}

/// The headers of a plugin set, for resolving includes between them.
#[derive(Debug)]
pub(crate) struct HeaderIndex {
    apps: BTreeSet<ApplicationName>,
    by_path: BTreeMap<(ApplicationName, String), PluginFile>,
    by_basename: BTreeMap<String, Vec<PluginFile>>,
}

impl HeaderIndex {
    pub(crate) fn new<'a>(
        apps: impl IntoIterator<Item = &'a ApplicationName>,
        headers: impl IntoIterator<Item = &'a PluginFile>,
    ) -> Self {
        let mut by_path = BTreeMap::new();
        let mut by_basename: BTreeMap<String, Vec<PluginFile>> = BTreeMap::new();
        for file in headers {
            by_path.insert(
                (file.app.clone(), file.path.as_str().to_owned()),
                file.clone(),
            );
            if let Some(base) = file.path.as_str().rsplit('/').next() {
                by_basename
                    .entry(base.to_owned())
                    .or_default()
                    .push(file.clone());
            }
        }
        Self {
            apps: apps.into_iter().cloned().collect(),
            by_path,
            by_basename,
        }
    }

    fn local(&self, app: &ApplicationName, path: &str) -> Option<&PluginFile> {
        self.by_path.get(&(app.clone(), path.to_owned()))
    }

    fn resolve(&self, from: &PluginFile, kind: &str, raw: &str) -> Target {
        if kind == "include_lib" {
            let (app, rest) = raw.split_once('/').unwrap_or((raw, ""));
            if let Ok(app) = ApplicationName::from_str(app) {
                if self.apps.contains(&app) {
                    return self
                        .local(&app, rest)
                        .map_or(Target::Unresolved, |file| Target::Local(file.clone()));
                }
                if let Ok(app_path) = RelativePath::new(rest) {
                    return Target::Host(HeaderRef { app, app_path });
                }
            }
            return Target::Unresolved;
        }
        let dir = from
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir);
        let joined = normalise(&format!("{dir}/{raw}"));
        let found = self
            .local(&from.app, &joined)
            .or_else(|| self.local(&from.app, &normalise(raw)))
            .or_else(|| {
                let base = raw.rsplit('/').next()?;
                let candidates = self.by_basename.get(base)?;
                candidates
                    .iter()
                    .find(|file| file.app == from.app)
                    .or_else(|| candidates.first())
            });
        found.map_or(Target::Unresolved, |file| Target::Local(file.clone()))
    }

    pub(crate) fn includes_of(&self, from: &PluginFile, code: &Code) -> Vec<Include> {
        INCLUDE
            .captures_iter(code.stripped())
            .map(|caps| {
                let whole = caps.get(0).expect("capture");
                Include {
                    line: code.line_at(whole.start()),
                    target: self.resolve(from, &caps[1], &caps[2]),
                }
            })
            .collect()
    }
}

/// Resolves `.` and `..` segments.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}
