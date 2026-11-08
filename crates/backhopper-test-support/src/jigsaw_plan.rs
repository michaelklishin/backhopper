// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! A loaded `jigsaw` plan over hand-built snapshots, written to a
//! temporary store and read back the way `jigsaw check` reads it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tempfile::TempDir;

use backhopper_core::config::{Config, ConfigFile};
use backhopper_core::erlang_mk::DepPin;
use backhopper_core::jigsaw::{
    ListedTags, LoadedPlan, PlanError, ResolvedRelease, SeriesRequest, TagSourceKind,
    host_facts_digest, plan_series,
};
use backhopper_core::model::names::{
    CommitSha, DependencyName, DependencyVersion, ExtractionSettingsDigest, ExtractorVersion,
    TagName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::release_version::ErlangMajor;
use backhopper_core::model::snapshot::{HostReleaseFacts, Snapshot, VendoredDepSource, state};
use backhopper_core::snapshot::freshness::{ExpectedExtraction, extraction_settings_digest};
use backhopper_core::store::SnapshotStore;

use crate::{HOST_CONFIG_TOML, SnapshotBuilder};

const EXTRACTOR: &str = "9";
const DIGITS: [char; 8] = ['1', '2', '3', '4', '5', '6', '7', '8'];

fn commit(digit: char) -> CommitSha {
    CommitSha::new(digit.to_string().repeat(40)).unwrap()
}

fn tags(values: &[&str]) -> Vec<TagName> {
    values
        .iter()
        .map(|value| TagName::new(*value).unwrap())
        .collect()
}

fn ra_pin() -> DepPin {
    DepPin {
        name: DependencyName::new("ra").unwrap(),
        version: DependencyVersion::new("2.17.3").unwrap(),
        source: VendoredDepSource::Hex,
    }
}

fn with_content(
    content: &SnapshotBuilder,
    commit: CommitSha,
    host_facts: Option<HostReleaseFacts>,
    dep_pins: Vec<DepPin>,
    settings: ExtractionSettingsDigest,
) -> Snapshot<state::Canonical> {
    let built = content.clone().build();
    let mut header = built.header().clone();
    header.commit = commit;
    header.dep_pins = dep_pins;
    header.host_facts = host_facts;
    header.extraction_settings = Some(settings);
    Snapshot::from_extracted(header, built.modules().to_vec(), built.headers().to_vec())
        .with_apps(built.apps().to_vec())
        .with_cuttlefish(built.schemas().to_vec(), built.partials().to_vec())
        .into_canonical()
}

/// The `rabbitmq` host of `HOST_CONFIG_TOML` at series `4.2`: one host
/// snapshot per `(tag, content)`, all pinning the same `ra` snapshot, and
/// one OTP snapshot per `(tag, content)`. The newest host tag is the build
/// patch.
#[must_use]
pub fn loaded_plan(
    host: &[(&str, SnapshotBuilder)],
    otp: &[(&str, SnapshotBuilder)],
    majors: &[u32],
) -> LoadedPlan {
    loaded_plan_with_config(HOST_CONFIG_TOML, host, otp, majors)
}

/// `HOST_CONFIG_TOML` with `apps` as the OTP project's `exclude_apps`.
#[must_use]
pub fn host_config_excluding_otp_apps(apps: &[&str]) -> String {
    let list: Vec<String> = apps.iter().map(|app| format!("\"{app}\"")).collect();
    HOST_CONFIG_TOML.replacen(
        "layout = \"erlang_otp\"",
        &format!(
            "layout = \"erlang_otp\"\nexclude_apps = [{}]",
            list.join(", ")
        ),
        1,
    )
}

#[must_use]
pub fn loaded_plan_with_config(
    config: &str,
    host: &[(&str, SnapshotBuilder)],
    otp: &[(&str, SnapshotBuilder)],
    majors: &[u32],
) -> LoadedPlan {
    let raw: ConfigFile = toml::from_str(config).unwrap();
    let cfg = Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw).unwrap();
    let rabbitmq = cfg.hosts.first().unwrap();
    let extractor = ExtractorVersion::new(EXTRACTOR).unwrap();
    let dir = TempDir::new().unwrap();
    let store = SnapshotStore::open_mut(dir.path()).unwrap();
    let digits: BTreeMap<&str, char> = host.iter().map(|(tag, _)| *tag).zip(DIGITS).collect();
    for (tag, content) in host {
        let facts = content
            .clone()
            .build()
            .header()
            .host_facts
            .as_ref()
            .map(|built| HostReleaseFacts {
                release_roots: built.release_roots.clone(),
                digest: host_facts_digest(&rabbitmq.pins, &rabbitmq.release_roots),
            });
        let snapshot = with_content(
            content,
            commit(digits[tag]),
            facts,
            vec![ra_pin()],
            settings_of(&cfg, content),
        );
        store.write(&snapshot).unwrap();
    }
    let ra = SnapshotBuilder::new("ra", "v2.17.3").app("ra", &[]);
    store
        .write(&with_content(
            &ra,
            commit('d'),
            None,
            Vec::new(),
            settings_of(&cfg, &ra),
        ))
        .unwrap();
    for (_, content) in otp {
        store
            .write(&with_content(
                content,
                commit('d'),
                None,
                Vec::new(),
                settings_of(&cfg, content),
            ))
            .unwrap();
    }
    let majors: Vec<ErlangMajor> = majors
        .iter()
        .map(|m| ErlangMajor::try_from(*m).unwrap())
        .collect();
    let host_tags: Vec<&str> = host.iter().map(|(tag, _)| *tag).collect();
    let otp_tags: Vec<&str> = otp.iter().map(|(tag, _)| *tag).collect();
    let plan = plan_series(
        &cfg,
        rabbitmq,
        &SeriesRequest {
            series: "4.2".parse().unwrap(),
            up_to: None,
            majors: NonEmpty::try_from(majors).unwrap(),
        },
        &ListedTags {
            host: tags(&host_tags),
            otp: tags(&otp_tags),
        },
        TagSourceKind::Repository,
        extractor.clone(),
        |release| {
            Ok::<_, PlanError>(ResolvedRelease {
                commit: commit(digits[release.tag().as_str()]),
                pins: vec![ra_pin()],
            })
        },
    )
    .unwrap()
    .plan;
    let read_only = SnapshotStore::open(dir.path()).unwrap();
    plan.load(&read_only, |project| {
        Some(ExpectedExtraction {
            extractor: extractor.clone(),
            settings: extraction_settings_digest(cfg.project(project).unwrap()),
        })
    })
    .unwrap()
}

fn settings_of(cfg: &Config, content: &SnapshotBuilder) -> ExtractionSettingsDigest {
    let project = content.clone().build().header().project.clone();
    extraction_settings_digest(cfg.project(&project).unwrap())
}
