// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! C6: the first verdict-pipeline integration of `backhopper-cuttlefish`.
//! `backhopper-core` stays free of the parser crate: the CLI extracts
//! `IntroducedMapping` values from `CuttlefishFragment`s whose span
//! intersects an added region, and this module compares those values
//! against the pinned cuttlefish version and the target tree.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use crate::compat::suite_registration::word_boundary_contains;
use crate::compat::target_tree_index::TargetTreeIndex;
use crate::model::names::{ApplicationName, RelativePath, SchemaFeature};
use crate::model::verdict::Reason;
use crate::versions::version_cmp;

/// A cuttlefish `mapping` fragment introduced by the patch (its span
/// intersects an added region), reduced to the plain values the two
/// C6 checks need: no `backhopper-cuttlefish` type crosses into core.
#[derive(Debug, Clone)]
pub struct IntroducedMapping {
    pub schema_path: RelativePath,
    /// The mapping's conf key (second tuple element).
    pub conf_key: Option<String>,
    /// The mapping's target Erlang key (third tuple element), e.g.
    /// `"rabbit.message_interceptors"`.
    pub mapping_target: Option<String>,
    /// Attribute names from the mapping's fourth tuple element.
    pub attr_names: Vec<String>,
}

/// Mapping attributes gated on a minimum cuttlefish version.
pub const SCHEMA_FEATURE_FLOORS: &[(&str, &str)] = &[("alias", "3.7.0")];

/// C6.2: fire when an introduced mapping uses a floor-gated attribute
/// the pinned cuttlefish version predates. Silent with no cuttlefish
/// pin (`pinned_version: None`).
pub fn check_schema_feature_unsupported(
    mappings: &[IntroducedMapping],
    pinned_version: Option<&str>,
) -> Vec<Reason> {
    let Some(pinned) = pinned_version else {
        return Vec::new();
    };
    let mut reasons = Vec::new();
    for mapping in mappings {
        for attr in &mapping.attr_names {
            let Some((_, required)) = SCHEMA_FEATURE_FLOORS.iter().find(|(name, _)| name == attr)
            else {
                continue;
            };
            // `version_cmp` orders descending (newer first): `Greater`
            // here means `pinned` is older than `required`. Equal to
            // the floor is supported.
            if version_cmp(pinned, required) != Ordering::Greater {
                continue;
            }
            let Ok(feature) = SchemaFeature::new((*attr).clone()) else {
                continue;
            };
            reasons.push(Reason::SchemaFeatureUnsupportedOnPin {
                schema_path: mapping.schema_path.clone(),
                conf_key: mapping.conf_key.clone(),
                feature,
                required_version: (*required).to_owned(),
                pinned_version: pinned.to_owned(),
            });
        }
    }
    reasons
}

/// C6.3: fire when an introduced mapping's target env key is not read
/// as a word-bounded token anywhere in the target tree's `.erl`
/// sources. All introduced target keys are checked against one cache
/// of blob reads, so a pick introducing eight mappings costs at most
/// one scan per distinct owning app, not eight.
pub fn check_schema_key_reader_missing(
    mappings: &[IntroducedMapping],
    target: &TargetTreeIndex,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<Reason> {
    let mut reasons = Vec::new();
    let mut app_source_cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for mapping in mappings {
        let Some(target_key) = &mapping.mapping_target else {
            continue;
        };
        let mut segments = target_key.split('.');
        let (Some(app), Some(env_key)) = (segments.next(), segments.next()) else {
            continue;
        };
        let app_name = ApplicationName::new(app).ok();
        let exists = app_has_any_path(target, app);
        let searched_apps: Vec<ApplicationName> = app_name.clone().into_iter().collect();
        let absent_apps: Vec<ApplicationName> = if exists {
            Vec::new()
        } else {
            app_name.into_iter().collect()
        };
        let found = exists && {
            let sources = app_source_cache
                .entry(app.to_owned())
                .or_insert_with(|| read_app_sources(target, app, read_target));
            sources
                .iter()
                .any(|src| word_boundary_contains(src, env_key))
        };
        if found {
            continue;
        }
        reasons.push(Reason::SchemaKeyReaderMissing {
            schema_path: mapping.schema_path.clone(),
            target_key: target_key.clone(),
            searched_apps,
            absent_apps,
        });
    }
    reasons
}

/// True when some present target-tree path lives under `deps/<app>/`
/// or a top-level `<app>/`: the app is real evidence of the app's
/// presence, cheap because `present_paths` is already in memory.
fn app_has_any_path(target: &TargetTreeIndex, app: &str) -> bool {
    let under_deps = format!("deps/{app}/");
    let top_level = format!("{app}/");
    target.present_paths().iter().any(|p| {
        let s = p.to_string_lossy();
        s.starts_with(under_deps.as_str()) || s.starts_with(top_level.as_str())
    })
}

/// Every `.erl` blob under the app's `src/` directory, read once.
fn read_app_sources(
    target: &TargetTreeIndex,
    app: &str,
    read_target: &dyn Fn(&RelativePath) -> Option<String>,
) -> Vec<String> {
    let under_deps = format!("deps/{app}/src/");
    let top_level = format!("{app}/src/");
    target
        .present_paths()
        .iter()
        .filter(|p| {
            let s = p.to_string_lossy();
            (s.starts_with(under_deps.as_str()) || s.starts_with(top_level.as_str()))
                && s.ends_with(".erl")
        })
        .filter_map(|p: &std::path::PathBuf| relative_path_of(p))
        .filter_map(|p| read_target(&p))
        .collect()
}

fn relative_path_of(p: &Path) -> Option<RelativePath> {
    RelativePath::new(p.to_str()?.to_owned()).ok()
}
