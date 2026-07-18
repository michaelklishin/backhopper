// Copyright (C) 2026 Michael S. Klishin and Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// See LICENSE-APACHE and LICENSE-MIT for details.

use std::borrow::Cow;
use std::str::FromStr;

use backhopper_driver::{UnknownVerb, Verb, VerbId};

#[test]
fn cli_path_round_trip_via_parse_for_every_verb() {
    for verb in Verb::iter() {
        let path = verb.cli_path().join(" ");
        let parsed = Verb::parse(&path).expect("known verb path parses");
        assert_eq!(parsed, verb, "round-trip failed for {verb:?}");
    }
}

#[test]
fn display_name_matches_cli_path() {
    assert_eq!(Verb::CheckPatch.display_name(), "check patch");
    assert_eq!(format!("{}", Verb::Version), "version");
}

#[test]
fn parse_accepts_slash_and_whitespace_separators() {
    assert_eq!(Verb::parse("check/patch").unwrap(), Verb::CheckPatch);
    assert_eq!(Verb::parse("check patch").unwrap(), Verb::CheckPatch);
    assert_eq!(Verb::parse("check\tpatch").unwrap(), Verb::CheckPatch);
    assert_eq!(Verb::parse("check  patch").unwrap(), Verb::CheckPatch);
}

#[test]
fn parse_rejects_unknown_paths() {
    let err = Verb::parse("nonsense verb").expect_err("unknown verb is rejected");
    let UnknownVerb(reported) = err;
    assert_eq!(reported, "nonsense verb");
}

#[test]
fn from_str_delegates_to_parse() {
    assert_eq!(
        Verb::from_str("snapshots verify").unwrap(),
        Verb::SnapshotsVerify
    );
}

#[test]
fn serde_round_trip_uses_kebab_case() {
    let s = serde_json::to_string(&Verb::CheckBatch).unwrap();
    assert_eq!(s, "\"check-batch\"");
    let back: Verb = serde_json::from_str(&s).unwrap();
    assert_eq!(back, Verb::CheckBatch);
}

#[test]
fn verb_id_known_into_owned_preserves_variant() {
    let id: VerbId<'static> = VerbId::from(Verb::CheckMerge).into_owned();
    assert!(matches!(id, VerbId::Known(Verb::CheckMerge)));
}

#[test]
fn verb_id_custom_carries_arbitrary_path() {
    let id = VerbId::Custom(Cow::Owned(vec![
        Cow::Borrowed("future"),
        Cow::Borrowed("experiment"),
    ]));
    assert_eq!(id.cli_path(), vec!["future", "experiment"]);
    assert_eq!(id.display_name(), "future experiment");
    assert!(id.as_known().is_none());
}

#[test]
fn verb_id_custom_into_owned_detaches_lifetime() {
    let s = String::from("future");
    let path = vec![Cow::Owned::<str>(s)];
    let id = VerbId::Custom(Cow::Owned(path));
    let owned = id.into_owned();
    assert_eq!(owned.display_name(), "future");
}

#[test]
fn iter_yields_every_documented_verb() {
    let count = Verb::iter().count();
    assert!(
        count >= 20,
        "iter() must enumerate every variant; got {count}"
    );
}

#[test]
fn schema_show_round_trips_via_parse() {
    assert_eq!(Verb::SchemaShow.cli_path(), &["schema", "show"]);
    assert_eq!(Verb::SchemaShow.display_name(), "schema show");
    assert_eq!(Verb::parse("schema show").unwrap(), Verb::SchemaShow);
}

#[test]
fn schema_diff_round_trips_via_parse() {
    assert_eq!(Verb::SchemaDiff.cli_path(), &["schema", "diff"]);
    assert_eq!(Verb::SchemaDiff.display_name(), "schema diff");
    assert_eq!(Verb::parse("schema diff").unwrap(), Verb::SchemaDiff);
}

#[test]
fn iter_includes_schema_subverbs() {
    let all: Vec<_> = Verb::iter().collect();
    assert!(all.contains(&Verb::SchemaShow));
    assert!(all.contains(&Verb::SchemaDiff));
}

#[test]
fn accepts_global_repo_dir_is_a_superset_of_requires() {
    for verb in Verb::iter() {
        if verb.requires_global_repo_dir() {
            assert!(
                verb.accepts_global_repo_dir(),
                "{verb} requires repo dir but does not accept it"
            );
        }
    }
}

#[test]
fn repo_free_verbs_do_not_accept_repo_dir_or_dry_run() {
    for verb in [
        Verb::Version,
        Verb::Doctor,
        Verb::SnapshotsList,
        Verb::CacheStats,
    ] {
        assert!(!verb.accepts_global_repo_dir(), "{verb}");
        assert!(!verb.accepts_dry_run(), "{verb}");
    }
}

#[test]
fn only_snapshot_write_verbs_accept_dry_run() {
    for verb in Verb::iter() {
        let expected = matches!(
            verb,
            Verb::SnapshotsGenerate | Verb::SnapshotsRebuild | Verb::SnapshotsMigrate
        );
        assert_eq!(verb.accepts_dry_run(), expected, "{verb}");
    }
}

#[test]
fn resolved_verb_parses_known_and_custom_paths() {
    assert_eq!(
        VerbId::Known(Verb::CheckBatch).resolved_verb(),
        Some(Verb::CheckBatch)
    );
    let custom = VerbId::Custom(Cow::Owned(vec![
        Cow::Borrowed("check"),
        Cow::Borrowed("batch"),
    ]));
    assert_eq!(custom.resolved_verb(), Some(Verb::CheckBatch));
    let unknown = VerbId::Custom(Cow::Owned(vec![
        Cow::Borrowed("future"),
        Cow::Borrowed("verb"),
    ]));
    assert_eq!(unknown.resolved_verb(), None);
}
