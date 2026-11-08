// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Helpers shared by the `jigsaw_*` unit test files: a host configuration,
//! stored host and OTP snapshots, and the extractor version they carry.

use std::path::{Path, PathBuf};

use backhopper_core::config::{Config, ConfigFile, Host};
use backhopper_core::erlang_mk::DepPin;
use backhopper_core::jigsaw::host_facts_digest;
use backhopper_core::model::app_manifest::{AppManifest, ManifestDeps, ManifestSource};
use backhopper_core::model::names::{
    ApplicationName, CommitSha, DependencyName, DependencyVersion, ExtractionSettingsDigest,
    ExtractorVersion, ProjectName, TagName,
};
use backhopper_core::model::non_empty::NonEmpty;
use backhopper_core::model::snapshot::{
    FORMAT_VERSION, HostReleaseFacts, Snapshot, VendoredDepSource, state,
};
use backhopper_core::snapshot::freshness::{ExpectedExtraction, extraction_settings_digest};
use backhopper_core::store::{Mutable, SnapshotStore};
use backhopper_test_support::{HOST_CONFIG_TOML, snapshot_header};

pub(crate) const EXTRACTOR: &str = "9";

pub(crate) const CONFIG: &str = HOST_CONFIG_TOML;

pub(crate) fn config_at(config_path: &Path, toml_text: &str) -> Config {
    let raw: ConfigFile = toml::from_str(toml_text).unwrap();
    Config::from_raw(config_path.to_path_buf(), raw).unwrap()
}

pub(crate) fn config() -> Config {
    config_at(&PathBuf::from("/tmp/backhopper.toml"), CONFIG)
}

pub(crate) fn rabbitmq(config: &Config) -> &Host {
    config.hosts.first().unwrap()
}

pub(crate) fn tag(value: &str) -> TagName {
    TagName::new(value).unwrap()
}

pub(crate) fn project(value: &str) -> ProjectName {
    ProjectName::new(value).unwrap()
}

pub(crate) fn commit(digit: char) -> CommitSha {
    CommitSha::new(digit.to_string().repeat(40)).unwrap()
}

pub(crate) fn tags(values: &[&str]) -> Vec<TagName> {
    values.iter().map(|value| tag(value)).collect()
}

pub(crate) fn extractor() -> ExtractorVersion {
    ExtractorVersion::new(EXTRACTOR).unwrap()
}

pub(crate) fn hex_pin(name: &str, version: &str) -> DepPin {
    DepPin {
        name: DependencyName::new(name).unwrap(),
        version: DependencyVersion::new(version).unwrap(),
        source: VendoredDepSource::Hex,
    }
}

pub(crate) fn app(name: &str) -> AppManifest {
    AppManifest {
        name: ApplicationName::new(name).unwrap(),
        source: ManifestSource::AppSrc,
        requires: ManifestDeps::Listed(Default::default()),
    }
}

pub(crate) fn settings_digest(project: &str) -> ExtractionSettingsDigest {
    extraction_settings_digest(config().project(&self::project(project)).unwrap())
}

pub(crate) fn expected(project: &ProjectName) -> Option<ExpectedExtraction> {
    Some(ExpectedExtraction {
        extractor: extractor(),
        settings: settings_digest(project.as_str()),
    })
}

pub(crate) struct StoredSnapshot<'a> {
    pub(crate) project: &'a str,
    pub(crate) tag: &'a str,
    pub(crate) commit: CommitSha,
    pub(crate) extractor: &'a str,
    pub(crate) format_version: u32,
    pub(crate) dep_pins: Vec<DepPin>,
    pub(crate) host_facts: Option<HostReleaseFacts>,
    pub(crate) settings: Option<ExtractionSettingsDigest>,
    pub(crate) apps: Vec<&'a str>,
}

impl<'a> StoredSnapshot<'a> {
    pub(crate) fn dependency(project: &'a str, tag: &'a str, app: &'a str) -> Self {
        Self {
            project,
            tag,
            commit: commit('d'),
            extractor: EXTRACTOR,
            format_version: FORMAT_VERSION,
            dep_pins: Vec::new(),
            host_facts: None,
            settings: Some(settings_digest(project)),
            apps: vec![app],
        }
    }

    pub(crate) fn otp(tag: &'a str) -> Self {
        Self::dependency("otp", tag, "kernel")
    }

    pub(crate) fn host(
        config: &Config,
        tag: &'a str,
        commit: CommitSha,
        pins: Vec<DepPin>,
    ) -> Self {
        let host = rabbitmq(config);
        Self {
            project: "rabbitmq-server",
            tag,
            commit,
            extractor: EXTRACTOR,
            format_version: FORMAT_VERSION,
            dep_pins: pins,
            host_facts: Some(HostReleaseFacts {
                release_roots: NonEmpty::new(ApplicationName::new("rabbit").unwrap(), []),
                digest: host_facts_digest(&host.pins, &host.release_roots),
            }),
            settings: Some(settings_digest("rabbitmq-server")),
            apps: vec!["rabbit"],
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot<state::Canonical> {
        let mut header = snapshot_header(self.project, self.tag);
        header.commit = self.commit.clone();
        self.extractor.clone_into(&mut header.extractor_version);
        header.format_version = self.format_version;
        header.dep_pins.clone_from(&self.dep_pins);
        header.host_facts.clone_from(&self.host_facts);
        header.extraction_settings.clone_from(&self.settings);
        Snapshot::from_extracted(header, Vec::new(), Vec::new())
            .with_apps(self.apps.iter().map(|name| app(name)).collect())
            .into_canonical()
    }

    pub(crate) fn write(&self, store: &SnapshotStore<Mutable>) {
        store.write(&self.snapshot()).unwrap();
    }
}
