// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A reader for erlang.mk's variable lines in a `Makefile`.
//!
//! It reads assignments (`=`, `:=`, `::=`, `?=`, `+=`) with `\` line
//! continuations and ignores everything else, including `include` lines
//! and conditionals. It is not a `make` implementation: a value that
//! references another variable is reported as unexpanded instead of
//! being evaluated.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;

use crate::errors::NameError;
use crate::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource, ManifestUnreadable};
use crate::model::names::{ApplicationName, DependencyName, DependencyVersion, MakeVariableName};
pub use crate::model::snapshot::{VendoredDep as DepPin, VendoredDepSource as DepSource};

const ASSIGNMENT_OPERATORS: [&str; 5] = ["::=", ":=", "?=", "+=", "="];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableValue {
    Words(Vec<String>),
    Unreadable(ManifestUnreadable),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MakeVariables {
    values: BTreeMap<MakeVariableName, VariableValue>,
}

impl MakeVariables {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut variables = Self::default();
        let mut in_define = false;
        for line in logical_lines(text) {
            let trimmed = line.trim();
            if in_define {
                in_define = trimmed != "endef";
                continue;
            }
            if trimmed.starts_with("define ") {
                in_define = true;
                continue;
            }
            if line.starts_with('\t') {
                continue;
            }
            if let Some(assignment) = Assignment::parse(trimmed) {
                variables.apply(assignment);
            }
        }
        variables
    }

    #[must_use]
    pub fn get<Q>(&self, name: &Q) -> Option<&VariableValue>
    where
        MakeVariableName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.values.get(name)
    }

    pub fn words<Q>(&self, name: &Q) -> Result<&[String], ManifestUnreadable>
    where
        MakeVariableName: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        match self.values.get(name) {
            Some(VariableValue::Words(words)) => Ok(words),
            Some(VariableValue::Unreadable(reason)) => Err(reason.clone()),
            None => Err(ManifestUnreadable::Missing),
        }
    }

    fn apply(&mut self, assignment: Assignment) {
        let Assignment {
            name,
            operator,
            value,
        } = assignment;
        let previous = self.values.get_mut(&name);
        match (operator, previous) {
            (Operator::IfUnset, Some(_)) => {}
            (Operator::Append, Some(VariableValue::Words(words))) => match value {
                VariableValue::Words(more) => words.extend(more),
                unreadable @ VariableValue::Unreadable(_) => {
                    self.values.insert(name, unreadable);
                }
            },
            (Operator::Append, Some(VariableValue::Unreadable(_))) => {}
            _ => {
                self.values.insert(name, value);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operator {
    Replace,
    IfUnset,
    Append,
}

struct Assignment {
    name: MakeVariableName,
    operator: Operator,
    value: VariableValue,
}

impl Assignment {
    fn parse(line: &str) -> Option<Self> {
        let name_end = line
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .unwrap_or(line.len());
        let name = MakeVariableName::from_str(&line[..name_end]).ok()?;
        let after = line[name_end..].trim_start();
        let operator_text = ASSIGNMENT_OPERATORS
            .iter()
            .find(|op| after.starts_with(**op))?;
        let operator = match *operator_text {
            "?=" => Operator::IfUnset,
            "+=" => Operator::Append,
            _ => Operator::Replace,
        };
        let value = read_value(&after[operator_text.len()..]);
        Some(Self {
            name,
            operator,
            value,
        })
    }
}

fn read_value(text: &str) -> VariableValue {
    let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    match words.iter().find_map(|w| first_reference(w)) {
        Some(Ok(name)) => VariableValue::Unreadable(ManifestUnreadable::UnexpandedVariable(name)),
        Some(Err(_)) => VariableValue::Unreadable(ManifestUnreadable::ParseFailed),
        None => VariableValue::Words(words),
    }
}

fn first_reference(word: &str) -> Option<Result<MakeVariableName, NameError>> {
    let start = word.find("$(").or_else(|| word.find("${"))? + 2;
    let rest = &word[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .unwrap_or(rest.len());
    Some(MakeVariableName::from_str(&rest[..end]))
}

fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        let code = raw.split_once('#').map_or(raw, |(code, _)| code);
        if let Some(continued) = code.strip_suffix('\\') {
            current.push_str(continued);
            current.push(' ');
            continue;
        }
        current.push_str(code);
        out.push(std::mem::take(&mut current));
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What an erlang.mk `Makefile` declares about its app: `PROJECT`, and
/// `DEPS` with `LOCAL_DEPS` as the apps the runtime needs. `BUILD_DEPS`,
/// `TEST_DEPS` and `DEP_PLUGINS` are not needed at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErlangMkManifest {
    pub project: Option<ApplicationName>,
    pub requires: ManifestDeps,
}

impl ErlangMkManifest {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let variables = MakeVariables::parse(text);
        let project = variables
            .words("PROJECT")
            .ok()
            .and_then(|words| words.first())
            .and_then(|word| ApplicationName::from_str(word).ok());
        Self {
            project,
            requires: read_requires(&variables),
        }
    }

    #[must_use]
    pub fn into_manifest(self, name: ApplicationName) -> AppManifest {
        AppManifest {
            name,
            source: ManifestSource::ErlangMk,
            requires: self.requires,
        }
    }
}

fn read_requires(variables: &MakeVariables) -> ManifestDeps {
    let mut requires = BTreeSet::new();
    for variable in ["DEPS", "LOCAL_DEPS"] {
        let words = match variables.words(variable) {
            Ok(words) => words,
            Err(ManifestUnreadable::Missing) => continue,
            Err(reason) => return ManifestDeps::Unreadable(reason),
        };
        for word in words {
            match ApplicationName::from_str(word) {
                Ok(app) => requires.insert(app),
                Err(_) => return ManifestDeps::Unreadable(ManifestUnreadable::ParseFailed),
            };
        }
    }
    ManifestDeps::Listed(requires)
}

static DEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*dep_([a-z0-9_]+)\s*=\s*(hex|git_rmq|git)\s+(.+?)\s*$").expect("regex")
});

impl DepPin {
    /// The envelope display form, e.g. `hex 2.17.1`.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{} {}", self.source.as_label(), self.version)
    }

    /// Inverse of `display`, for reading the form back out of an envelope.
    #[must_use]
    pub fn parse_display(name: &DependencyName, s: &str) -> Option<Self> {
        let (label, version) = s.split_once(' ')?;
        Some(Self {
            name: name.clone(),
            version: DependencyVersion::new(version).ok()?,
            source: DepSource::from_label(label)?,
        })
    }
}

/// A `dep_` line: a pin, or the name of one the name types refuse.
enum PinLine {
    Pin(DepPin),
    Refused(String),
}

fn scan_pin_line(line: &str) -> Option<PinLine> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let caps = DEP_RE.captures(line)?;
    let name = &caps[1];
    if name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo") {
        return None;
    }
    let source = DepSource::from_label(&caps[2])?;
    let rest = caps[3].trim();
    let mut words = rest.split_whitespace();
    let version = match source {
        DepSource::Hex => words.next()?,
        DepSource::Git | DepSource::GitRmq => words.nth(1)?,
    };
    Some(
        match (DependencyName::new(name), DependencyVersion::new(version)) {
            (Ok(name), Ok(version)) => PinLine::Pin(DepPin {
                name,
                version,
                source,
            }),
            _ => PinLine::Refused(name.to_owned()),
        },
    )
}

/// Parses one `dep_NAME = hex|git|git_rmq ...` line. A line whose name or
/// version the name types refuse is not a pin.
#[must_use]
pub fn parse_pin_line(line: &str) -> Option<DepPin> {
    match scan_pin_line(line)? {
        PinLine::Pin(pin) => Some(pin),
        PinLine::Refused(_) => None,
    }
}

/// The pins of a components file, and the names of the `dep_` lines the
/// name types refused, so a caller can report them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PinScan {
    pub pins: Vec<DepPin>,
    pub refused_names: Vec<String>,
}

#[must_use]
pub fn scan_components_mk(text: &str) -> PinScan {
    let mut scan = PinScan::default();
    for line in text.lines().filter_map(scan_pin_line) {
        match line {
            PinLine::Pin(pin) => scan.pins.push(pin),
            PinLine::Refused(name) => scan.refused_names.push(name),
        }
    }
    scan
}

#[must_use]
pub fn parse_components_mk(text: &str) -> Vec<DepPin> {
    scan_components_mk(text).pins
}

/// A hex version is a bare release number, so the project's tag prefix is
/// applied unless the version already carries it.
#[must_use]
pub fn version_to_tag(version: &str, tag_prefix: &str) -> String {
    if version.starts_with(tag_prefix) {
        version.to_owned()
    } else {
        format!("{tag_prefix}{version}")
    }
}

/// Hex deps get the project's tag prefix; git deps use the version verbatim.
#[must_use]
pub fn dep_to_tag(pin: &DepPin, tag_prefix: &str) -> String {
    match pin.source {
        DepSource::Hex => version_to_tag(pin.version.as_str(), tag_prefix),
        DepSource::Git | DepSource::GitRmq => pin.version.to_string(),
    }
}
