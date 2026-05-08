//! Parse RabbitMQ's `rabbitmq-components.mk` dep pins.
//!
//! Two real-world line shapes:
//!  * `dep_NAME = hex VERSION`            (e.g. `dep_ra = hex 3.1.6`)
//!  * `dep_NAME = git URL TAG`            (e.g. `dep_osiris = git https://... v1.13.1`)
//!  * `dep_NAME = git_rmq NAME TAG`       (vendored fork: `dep_cowboy = git_rmq cowboy 2.13.0`)
//!
//! `hex` versions are bare (`3.1.6`); `git`/`git_rmq` versions are real
//! tag names. Callers map a `hex` version back to a tag by applying the
//! project's `tag_prefix` from config.

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepPin {
    pub name: String,
    pub version: String,
    pub source: DepSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepSource {
    Hex,
    Git,
    GitRmq,
}

impl DepSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Git => "git",
            Self::GitRmq => "git_rmq",
        }
    }
}

fn dep_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^\s*dep_([a-z0-9_]+)\s*=\s*(hex|git_rmq|git)\s+(.+?)\s*$").expect("regex")
    })
}

pub fn parse_components_mk(text: &str) -> Vec<DepPin> {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some(caps) = dep_re().captures(line) else {
            continue;
        };
        let name = caps[1].to_owned();
        if name.ends_with("_commit") || name.ends_with("_branch") || name.ends_with("_repo") {
            continue;
        }
        let source_label = &caps[2];
        let rest = caps[3].trim();
        let (source, version) = match source_label {
            "hex" => (
                DepSource::Hex,
                rest.split_whitespace().next().unwrap_or("").to_owned(),
            ),
            "git" => match parse_git_url_tag(rest) {
                Some(v) => (DepSource::Git, v),
                None => continue,
            },
            "git_rmq" => match parse_git_rmq(rest) {
                Some(v) => (DepSource::GitRmq, v),
                None => continue,
            },
            _ => continue,
        };
        if version.is_empty() {
            continue;
        }
        out.push(DepPin {
            name,
            version,
            source,
        });
    }
    out
}

fn parse_git_url_tag(rest: &str) -> Option<String> {
    let mut parts = rest.split_whitespace();
    let _url = parts.next()?;
    let tag = parts.next()?;
    Some(tag.to_owned())
}

fn parse_git_rmq(rest: &str) -> Option<String> {
    let mut parts = rest.split_whitespace();
    let _name = parts.next()?;
    let tag = parts.next()?;
    Some(tag.to_owned())
}

pub fn series_name_for_branch(branch: &str) -> String {
    let trimmed = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let trimmed = trimmed.strip_prefix("refs/tags/").unwrap_or(trimmed);
    if trimmed == "main" || trimmed == "master" {
        return format!("rabbitmq-{}", trimmed);
    }
    let stripped = trimmed.strip_suffix(".x").unwrap_or(trimmed);
    let stripped = stripped.strip_prefix('v').unwrap_or(stripped);
    format!("rabbitmq-{}", stripped)
}

pub fn version_to_tag(version: &str, tag_prefix: &str) -> String {
    if version.starts_with(tag_prefix) {
        version.to_owned()
    } else {
        format!("{}{}", tag_prefix, version)
    }
}
