// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use backhopper_core::config::{
    Config, ConfigFile, PathTranslationRaw, PathTranslations, TranslationDirection,
};
use backhopper_core::errors::ConfigError;

fn parse(body: &str) -> Result<Config, ConfigError> {
    let raw: ConfigFile = toml::from_str(body).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), raw)
}

#[test]
fn parses_minimal_path_translation_stanza() {
    let cfg = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "fed_split"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
"#,
    )
    .expect("loads");
    assert_eq!(cfg.path_translations.entries.len(), 1);
    let entry = &cfg.path_translations.entries[0];
    assert_eq!(entry.name, "fed_split");
    assert_eq!(entry.direction, TranslationDirection::SourceToTarget);
}

#[test]
fn rejects_empty_source_prefix() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "x"
source_prefix = ""
target_prefix = "deps/a/"
"#,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ConfigError::PathTranslationEmptyPrefix { .. }
    ));
}

#[test]
fn rejects_missing_trailing_slash() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "x"
source_prefix = "deps/a"
target_prefix = "deps/b/"
"#,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ConfigError::PathTranslationPrefixNoTrailingSlash { .. }
    ));
}

#[test]
fn rejects_duplicate_name() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "x"
source_prefix = "deps/a/"
target_prefix = "deps/b/"

[[path_translation]]
name = "x"
source_prefix = "deps/c/"
target_prefix = "deps/d/"
"#,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::PathTranslationDuplicateName(_)));
}

#[test]
fn rejects_duplicate_source_prefix() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "a"
source_prefix = "deps/x/"
target_prefix = "deps/y/"

[[path_translation]]
name = "b"
source_prefix = "deps/x/"
target_prefix = "deps/z/"
"#,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ConfigError::PathTranslationDuplicateSource(_)
    ));
}

#[test]
fn rejects_prefix_overlap() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "a"
source_prefix = "deps/"
target_prefix = "deps/legacy/"

[[path_translation]]
name = "b"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
"#,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ConfigError::PathTranslationPrefixOverlap { .. }
    ));
}

#[test]
fn rejects_unknown_direction() {
    let err = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "x"
direction = "target_to_source"
source_prefix = "deps/a/"
target_prefix = "deps/b/"
"#,
    )
    .unwrap_err();
    assert!(matches!(
        err,
        ConfigError::PathTranslationUnknownDirection { .. }
    ));
}

#[test]
fn translate_prefix_swaps_path() {
    let body = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "fed_split"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
"#;
    let cfg = parse(body).unwrap();
    let (rewritten, entry) = cfg
        .path_translations
        .translate("deps/rabbitmq_federation_common/src/foo.erl")
        .expect("matches");
    assert_eq!(
        rewritten.to_string_lossy(),
        "deps/rabbitmq_federation/src/foo.erl"
    );
    assert_eq!(entry.name, "fed_split");
}

#[test]
fn translate_returns_none_when_no_prefix_matches() {
    let cfg = parse(
        r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "fed_split"
source_prefix = "deps/rabbitmq_federation_common/"
target_prefix = "deps/rabbitmq_federation/"
"#,
    )
    .unwrap();
    assert!(cfg.path_translations.translate("README.md").is_none());
}

#[test]
fn external_file_missing_is_hard_error() {
    let err = PathTranslations::load_external(Path::new("/nonexistent/path/translations.toml"))
        .unwrap_err();
    assert!(matches!(err, ConfigError::PathTranslationFileMissing(_)));
}

#[test]
fn merge_external_combines_distinct_entries() {
    let mut pt = PathTranslations::default();
    let raw_a = vec![PathTranslationRaw {
        name: "a".to_string(),
        direction: "source_to_target".to_string(),
        source_prefix: "deps/x/".to_string(),
        target_prefix: "deps/y/".to_string(),
    }];
    let a = PathTranslations::from_config_stanzas(raw_a).unwrap();
    pt.entries.extend(a.entries);

    let body = r#"
[[path_translation]]
name = "b"
source_prefix = "deps/p/"
target_prefix = "deps/q/"
"#;
    let f = NamedTempFile::new().unwrap();
    fs::write(f.path(), body).unwrap();
    let external = PathTranslations::load_external(f.path()).unwrap();
    pt.merge_external(external).unwrap();

    assert_eq!(pt.entries.len(), 2);
    assert!(pt.entries.iter().any(|e| e.name == "a"));
    assert!(pt.entries.iter().any(|e| e.name == "b"));
}

#[test]
fn merge_external_rejects_duplicate_name() {
    let raw_a = vec![PathTranslationRaw {
        name: "fed_split".to_string(),
        direction: "source_to_target".to_string(),
        source_prefix: "deps/x/".to_string(),
        target_prefix: "deps/y/".to_string(),
    }];
    let mut pt = PathTranslations::from_config_stanzas(raw_a).unwrap();
    let body = r#"
[[path_translation]]
name = "fed_split"
source_prefix = "deps/p/"
target_prefix = "deps/q/"
"#;
    let f = NamedTempFile::new().unwrap();
    fs::write(f.path(), body).unwrap();
    let external = PathTranslations::load_external(f.path()).unwrap();
    let err = pt.merge_external(external).unwrap_err();
    assert!(matches!(err, ConfigError::PathTranslationDuplicateName(_)));
}
