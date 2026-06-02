// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

//! Invariants for `PathTranslations::translate` and the
//! `target_tree::normalise` path canonicaliser.

use std::path::{Path, PathBuf};

use proptest::prelude::*;

use backhopper_core::compat::normalise;
use backhopper_core::config::{
    Config, ConfigFile, PathTranslations, TranslationDirection, TranslationOrigin,
};

fn safe_seg() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,5}".prop_map(String::from)
}

fn safe_rel_path() -> impl Strategy<Value = String> {
    proptest::collection::vec(safe_seg(), 1..5).prop_map(|s| s.join("/"))
}

fn one_translation() -> PathTranslations {
    let raw = r#"
config_version = 1
[defaults]
fallback_branch = "main"

[[path_translation]]
name = "t"
source_prefix = "deps/src_x/"
target_prefix = "deps/dst_x/"
"#;
    let cf: ConfigFile = toml::from_str(raw).unwrap();
    Config::from_raw(PathBuf::from("/tmp/backhopper.toml"), cf)
        .unwrap()
        .path_translations
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn translate_only_rewrites_when_prefix_matches(suffix in safe_rel_path()) {
        let pt = one_translation();
        let with_prefix = format!("deps/src_x/{suffix}");
        let (rewritten, entry) = pt.translate(&with_prefix).expect("matches");
        prop_assert_eq!(rewritten.to_string_lossy(), format!("deps/dst_x/{suffix}"));
        prop_assert_eq!(&entry.name, "t");
        prop_assert_eq!(entry.direction, TranslationDirection::SourceToTarget);
        prop_assert!(matches!(entry.origin, TranslationOrigin::ConfigStanza));
    }

    #[test]
    fn translate_returns_none_for_unrelated_prefix(p in safe_rel_path()) {
        let pt = one_translation();
        prop_assume!(!p.starts_with("deps/src_x/"));
        let unrelated = format!("other_root/{p}");
        prop_assert!(pt.translate(&unrelated).is_none());
    }

    #[test]
    fn normalise_strips_dot_slash_prefixes(seg in safe_rel_path()) {
        let prefixed = format!("./{seg}");
        let n = normalise(Path::new(&prefixed)).expect("safe path");
        prop_assert_eq!(n.to_string_lossy(), seg);
    }

    #[test]
    fn normalise_is_idempotent(p in safe_rel_path()) {
        let once = normalise(Path::new(&p)).unwrap();
        let twice = normalise(&once).unwrap();
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn normalise_rejects_dotdot_anywhere(prefix in safe_rel_path(), suffix in safe_rel_path()) {
        let with_dd = format!("{prefix}/../{suffix}");
        prop_assert!(normalise(Path::new(&with_dd)).is_none());
    }

    #[test]
    fn translate_preserves_suffix_byte_for_byte(suffix in safe_rel_path()) {
        let pt = one_translation();
        let with_prefix = format!("deps/src_x/{suffix}");
        let (rewritten, _) = pt.translate(&with_prefix).unwrap();
        let target_str = rewritten.to_string_lossy();
        prop_assert!(target_str.ends_with(&*suffix));
        prop_assert!(target_str.starts_with("deps/dst_x/"));
    }
}
